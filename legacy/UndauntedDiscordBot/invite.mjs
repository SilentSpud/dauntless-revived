import { readFile } from 'node:fs/promises';

export async function loadInviteConfig(file) {
  if (!file) throw new Error('Configure SERVER_CONFIG_FILE for launcher invites');
  const config = JSON.parse(await readFile(file, 'utf8'));
  // Validate before the bot starts accepting claims.
  launcherInvite(config, 'VALIDATION');
  return config;
}

export function launcherInvite(config, code) {
  if (!/^[A-Za-z0-9-]{4,64}$/.test(code)) throw new Error('Registration code is not launcher compatible');
  if (config.Mode !== 'Public' || !/^[a-z0-9.-]{1,253}$/i.test(config.PublicHost || '') || !Number.isInteger(config.Ports?.gateway) || config.Ports.gateway < 1 || config.Ports.gateway > 65535 || !/^[a-f0-9]{64}$/i.test(config.CertFingerprint || '')) throw new Error('Invalid public server configuration');
  const query = new URLSearchParams({v:'2',mode:'public',host:config.PublicHost.toLowerCase(),port:String(config.Ports.gateway),fp:config.CertFingerprint.toLowerCase(),code,name:config.ServerName || 'Dauntless Revived'});
  return `dauntless-revived://join?${query}`;
}

export function inviteMessage(config, code) {
  return `🔑 **Your Revived Invite**\n\`\`\`\n${launcherInvite(config, code)}\n\`\`\`\nCopy the entire invite above into the launcher's **Join** box, then choose your username on **Register**. The registration code is already included. The launcher saves your account key after registration.\nAlready registered? Keep your original launcher key; use /key link to link it to Discord.\n**Clear skies, Slayer.**`;
}
