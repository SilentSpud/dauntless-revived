import { logger } from "../logger";

// Shared checks for the writes only a game server may make. Used after HasUndauntedMetagameAuth, which
// sets req.AuthData: {IsGameserver: true} for the game-server key, plus the account of a player token
// that the game server sent along (userId).

// Game-server-only writes of real-mode accounts: entitlement grants and revokes, the Hunt Pass choice,
// cooldowns, bounties, loadout unlocks and the active loadout slot. A player's own client may not make
// them (the game server checks the conditions first). Answers 403 and returns true for a player's
// request. Subject is what the refusal log names; by default the account in the URL.
export function RefuseUnlessGameserver(req: any, res: any, What: string, Subject: string = String(req.params?.userId)){
    if(req.AuthData?.IsGameserver){
        NoteRelayedAccountMismatch(req, req.params?.userId, What);
        return false;
    }

    logger.warn(`Refusing ${What} for ${Subject} from a player client`);
    res.status(403);
    res.send();
    return true;
}

// Once a minute at most for the same write, account and token owner
const NOTE_EVERY_MS = 60 * 1000;
const LastNoted = new Map<string, number>();

// A game server's request that carries a player's token for another account than the one it names.
// It is logged and the request goes on: the game server is trusted with the account it names, as it
// always was here. Harmonic's fork refuses such a write (403); if a 1.4.4 game server ever writes for
// one player with another player's token (not seen yet; the two-player test will show), a refusal would
// lose that player's save. Returns true when the request is such a mismatch.
export function NoteRelayedAccountMismatch(req: any, AccountId: unknown, What: string){
    const Relayed = req.AuthData?.userId;

    if(req.AuthData?.IsGameserver !== true || typeof Relayed !== "string" || Relayed.length === 0 || Relayed === AccountId){
        return false;
    }

    const Key = `${What}|${String(AccountId)}|${Relayed}`;
    const Now = Date.now();

    if(Now - (LastNoted.get(Key) ?? -Infinity) >= NOTE_EVERY_MS){
        if(LastNoted.size >= 1000){
            LastNoted.clear();
        }

        LastNoted.set(Key, Now);
        logger.warn(`Game server ${What} for ${String(AccountId)} carries the token of ${Relayed}: accepted for ${String(AccountId)}, the account the request names`);
    }

    return true;
}
