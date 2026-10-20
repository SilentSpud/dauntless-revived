import { randomBytes } from 'node:crypto';
import { mkdir, readFile, writeFile, rename } from 'node:fs/promises';
import { dirname } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { linkInput } from './link-input.mjs';

export async function loadState(file) {
  try {
    const state = JSON.parse(await readFile(file, 'utf8'));
    if (state.version !== 1 || !state.users || typeof state.users !== 'object' || Array.isArray(state.users)) throw new Error('Invalid key state');
    if (state.links !== undefined && (!state.links || typeof state.links !== 'object' || Array.isArray(state.links) ||
      Object.entries(state.links).some(([id, value]) => !/^\d{17,20}$/.test(id) || !value || typeof value.userId !== 'string' || typeof value.username !== 'string')))
      throw new Error('Invalid account link state');
    return state;
  } catch (error) {
    if (error.code === 'ENOENT') return { version: 1, users: {} };
    throw new Error('Cannot read key state; refusing to issue replacement codes');
  }
}

export async function saveState(file, state, {platform = process.platform, replace = rename, sleep = delay} = {}) {
  await mkdir(dirname(file), {recursive: true});
  await writeFile(`${file}.tmp`, JSON.stringify(state), {mode: 0o600});
  // Antivirus/indexers can briefly deny Windows replacement. Keep the original
  // file intact and retry the same atomic rename, never unlink/copy over it.
  for (let attempt = 0; ; ++attempt) {
    try { await replace(`${file}.tmp`, file); return; }
    catch (error) {
      if (platform !== 'win32' || !['EPERM', 'EACCES', 'EBUSY'].includes(error.code) || attempt >= 6) throw error;
      await sleep(50 * 2 ** attempt);
    }
  }
}

export function backend(base, key, request = fetch) {
  const url = new URL(base);
  if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || url.username || url.password || url.pathname !== '/' || url.search || url.hash) throw new Error('Metagame must use loopback HTTP');
  async function api(path, body) {
    const response = await request(new URL(`/undaunted/api/${path}`, url), {
      method: body ? 'POST' : 'GET', redirect: 'error', signal: AbortSignal.timeout(15000),
      headers: {'x-undaunted-user-api-key': key, 'content-type': 'application/json'},
      body: body ? JSON.stringify(body) : undefined,
    });
    if (!response.ok) throw Object.assign(new Error('Metagame request failed'), {status: response.status});
    return path === 'RegisterInviteCode' ? undefined : response.json();
  }
  return {
    async linkAccount(discordId, userId) { return api('DiscordLink', {DiscordId: discordId, UserId: userId}); },
    async linkedAccount(discordId) { return (await api(`DiscordLink/${discordId}`)).account; },
    async find(code) {
      const result = await api('InviteCodes');
      if (!Array.isArray(result.InviteCodes)) throw new Error('Invalid metagame response');
      return result.InviteCodes.find(row => row.inviteCode === code);
    },
    async create(code) { await api('RegisterInviteCode', {NewInviteCode: code, Uses: 1, InfiniteUses: false}); },
    async identity(launcherKey) {
      const response = await request(new URL('/undaunted/api/GetUserInfo', url), {
        headers: {'x-undaunted-user-api-key': launcherKey}, redirect: 'error', signal: AbortSignal.timeout(5000),
      });
      if (response.status === 401 || response.status === 403) return null;
      if (!response.ok) throw new Error('Account verification unavailable');
      const account = await response.json();
      if (typeof account.UserId !== 'string' || !/^UID-[A-Za-z0-9-]{1,100}$/.test(account.UserId) || typeof account.Username !== 'string')
        throw new Error('Unexpected account verification response');
      return {userId: account.UserId, username: account.Username};
    },
  };
}

export class Keys {
  queue = Promise.resolve();
  constructor(state, save, api, generate = () => `DR-${randomBytes(18).toString('hex')}`) {
    this.state = state; this.save = save; this.api = api; this.generate = generate;
  }
  async migrateLinks() {
    for (const [discordId, account] of Object.entries(this.state.links || {})) {
      const result = await this.api.linkAccount(discordId, account.userId);
      if (result.status !== 'linked') throw new Error('Legacy account link needs operator review');
    }
  }
  run(user, claim) {
    const task = this.queue.catch(() => {}).then(() => this.handle(user, claim));
    this.queue = task;
    return task;
  }
  link(user, launcherKey) {
    const task = this.queue.catch(() => {}).then(async () => {
      if (!/^\d{17,20}$/.test(user)) throw new Error('Invalid Discord user');
      const input = linkInput(launcherKey);
      if (!input.key) return input;
      const account = await this.api.identity(input.key);
      if (!account) return {status:input.key.startsWith('DR-') ? 'invite_not_key' : 'invalid_key'};
      return this.api.linkAccount(user, account.userId);
    });
    this.queue = task;
    return task;
  }
  deliverPrivate(user, reply) {
    const task = this.queue.catch(() => {}).then(async () => {
      const result = await this.handle(user, true);
      if (result.status !== 'ready') return result;
      // Re-display only this user's existing unused invite, including failed DMs.
      await reply(result.code);
      const entry = this.state.users[user];
      entry.delivery = 'sent';
      entry.deliveryChannel = 'ephemeral';
      entry.sentAt = new Date().toISOString();
      delete entry.lastDeliveryError;
      await this.save(this.state);
      return {status: 'private_sent'};
    });
    this.queue = task;
    return task;
  }
  deliver(user, send, reconcile) {
    const task = this.queue.catch(() => {}).then(async () => {
      const result = await this.handle(user, true);
      if (result.status !== 'ready') return result;
      const entry = this.state.users[user];
      // Previous private replies were dismissible; deliver that same invite once by DM.
      if (entry.deliveryChannel === 'ephemeral') entry.delivery = 'unsent';
      if (entry.delivery === 'sent') return {status: 'already_sent'};
      if (entry.delivery !== 'unsent') {
        if (!reconcile) return {status: entry.delivery === 'reserved' ? 'delivery_uncertain' : 'already_sent'};
        let previous;
        try { previous = await reconcile(result.code); }
        catch (error) {
          if (error.code === 50007) return {status:'dm_disabled'};
          if (error.code === 50278) return {status:'no_mutual_guild'};
          return {status:'delivery_uncertain'};
        }
        if (previous === undefined) return {status: 'delivery_uncertain'};
        if (previous) {
          entry.delivery = 'sent'; entry.deliveryChannel = 'dm'; entry.messageId = previous.id; entry.sentAt = previous.createdAt;
          await this.save(this.state);
          return {status: 'already_sent'};
        }
      }
      // Reserve durably before Discord: an ambiguous timeout must never send twice.
      entry.delivery = 'reserved';
      entry.deliveryChannel = 'dm';
      entry.attemptedAt = new Date().toISOString();
      try { await this.save(this.state); }
      catch (error) { entry.delivery = 'unsent'; throw error; }
      let message;
      try { message = await send(result.code); }
      catch (error) {
        // Discord explicitly rejected the DM; no message was delivered.
        if (error.code === 50007 || error.code === 50278) {
          entry.delivery = 'unsent';
          entry.lastDeliveryError = error.code;
          await this.save(this.state);
          return {status: error.code === 50278 ? 'no_mutual_guild' : 'dm_disabled'};
        }
        return {status: 'delivery_uncertain'};
      }
      entry.delivery = 'sent';
      delete entry.lastDeliveryError;
      entry.sentAt = new Date().toISOString();
      if (message?.id) entry.messageId = message.id;
      await this.save(this.state);
      return {status: 'sent'};
    });
    this.queue = task;
    return task;
  }
  async handle(user, claim) {
    if (!/^\d{17,20}$/.test(user)) throw new Error('Invalid Discord user');
    if (await this.api.linkedAccount(user)) return {status:'linked'};
    let entry = this.state.users[user];
    if (!entry) {
      if (!claim) return {status: 'none'};
      entry = this.state.users[user] = {code: this.generate(), pending: true, delivery: 'unsent', createdAt: new Date().toISOString()};
      // Save before contacting the backend: retries after a crash reuse this code.
      try { await this.save(this.state); }
      catch (error) { delete this.state.users[user]; throw error; }
    }
    let row = await this.api.find(entry.code);
    if (!row && entry.pending && claim) {
      await this.api.create(entry.code);
      row = await this.api.find(entry.code);
    }
    if (!row) return {status: entry.pending ? 'pending' : 'revoked'};
    if (row.infiniteUses || !Number.isInteger(row.usesRemaining) || row.usesRemaining < 0 || row.usesRemaining > 1) throw new Error('Unexpected code state');
    if (entry.pending) { entry.pending = false; await this.save(this.state); }
    return row.usesRemaining === 0 ? {status: 'redeemed'} : {status: 'ready', code: entry.code};
  }
}
