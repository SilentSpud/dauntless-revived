import { logger } from "./logger";

// The switches added with the port of Harmonic's 1.4.4 fork (github.com/Harmonicrain/Undaunted), read
// in one place. Each has a typed default; a value that does not parse is a warning (once per value) and
// the default. Values are read from the environment at every use, as the older switches are, so a test
// can change one; the boot line (DescribeFeatures) shows what a running server uses. Older switches
// (PROGRESSION_MODE, CHAT, LOG_BODIES, ...) keep their own parsing.
//
// On and off switches take 1, true, on or yes, and 0, false, off or no (any case).

type FeatureSwitch<T> = {
    Env: string,
    Label: string,       // its name in the boot line
    Default: T,
    Parse: (Raw: string) => T | undefined,
    Show: (Value: T) => string
};

const Switches: FeatureSwitch<any>[] = [];
const WarnedValues = new Set<string>();

function ReadSwitch<T>(Switch: FeatureSwitch<T>): T {
    const Raw = process.env[Switch.Env];

    if(Raw === undefined || Raw.trim().length === 0){
        return Switch.Default;
    }

    const Value = Switch.Parse(Raw.trim());

    if(Value === undefined){
        const Key = `${Switch.Env}=${Raw}`;

        if(!WarnedValues.has(Key)){
            WarnedValues.add(Key);
            logger.warn(`${Switch.Env}=${JSON.stringify(Raw)} is not a valid value; using the default (${Switch.Show(Switch.Default)})`);
        }

        return Switch.Default;
    }

    return Value;
}

function DefineSwitch<T>(Switch: FeatureSwitch<T>): () => T {
    Switches.push(Switch);

    return () => ReadSwitch(Switch);
}

export function ParseOnOff(Raw: string): boolean | undefined {
    const Value = Raw.toLowerCase();

    if(["1", "true", "on", "yes"].includes(Value)){
        return true;
    }

    if(["0", "false", "off", "no"].includes(Value)){
        return false;
    }

    return undefined;
}

// A whole number from 0 up
export function ParseCount(Raw: string): number | undefined {
    return /^\d{1,9}$/.test(Raw) ? Number(Raw) : undefined;
}

export function ParseChoice<T extends string>(Choices: readonly T[]): (Raw: string) => T | undefined {
    return (Raw) => Choices.find((Choice) => Choice === Raw.toLowerCase());
}

export const ShowOnOff = (Value: boolean) => Value ? "on" : "off";

// ---- The switches ----

// LOG_BODIES=1 only: at most this many body-log lines per method and path in one run of the metagame
// (a client stuck in a loop once wrote megabytes in minutes). 0, the default, is no cap.
export const BodyLogPerPath = DefineSwitch({
    Env: "BODY_LOG_PER_PATH",
    Label: "bodyLogPerPath",
    Default: 0,
    Parse: ParseCount,
    Show: (Value: number) => Value === 0 ? "no-cap" : String(Value)
});

// Escalation saves (roadmap 2.16; controllers/escalation.ts). stub, the default: every account reads
// upstream's fake maximum and the save route answers 404, as before. real: accounts in real progression
// mode read and save their own seasons; players drop from the fake maximum to level 0 and level up for
// real, so switching it on is the owner's decision after the in-game test.
export const EscalationMode = DefineSwitch({
    Env: "ESCALATION_MODE",
    Label: "escalation",
    Default: "stub" as "stub" | "real",
    Parse: ParseChoice(["stub", "real"] as const),
    Show: (Value: string) => Value
});

// ESCALATION_MODE=real only. The save rules that depend on how the talent tiers, the level costs and
// the reward levels are modelled: off (the default) logs a save that breaks one and stores it anyway;
// on refuses it (409). The rules every save must pass are always enforced.
export const EscalationStrict = DefineSwitch({
    Env: "ESCALATION_STRICT",
    Label: "escalationStrict",
    Default: false,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// The in-game store (roadmap 3.7; controllers/freestore.ts). off, the default: the store screen gets the
// old 400 and the purchase routes answer 404, as before. free: the catalogue in vendor/store_catalog.json,
// every offer free, bought with the client's two-step purchase token. Free or priced is the owner's
// decision (the economy question of 3.7), and the store waits for an in-game test.
export const StoreMode = DefineSwitch({
    Env: "STORE",
    Label: "store",
    Default: "off" as "off" | "free",
    Parse: ParseChoice(["off", "free"] as const),
    Show: (Value: string) => Value
});

// STORE=free only: list and sell the 20 premium bounty-token bundle any number of times (unlimited free
// premium bounty drafts). Off by default: the offer is not listed and cannot be bought.
export const StoreRepeatableTokens = DefineSwitch({
    Env: "STORE_REPEATABLE_TOKENS",
    Label: "storeRepeatableTokens",
    Default: false,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// Lady Luck's Trials store. Independent of STORE: the normal real-money/free-cosmetic storefront can
// remain off while Lady Luck uses the same recovered purchase-token service for Steel/Gilded Marks.
// On by default: the catalogue has been through a real 1.4.4 client.
export const TrialsStore = DefineSwitch({
    Env: "TRIALS_STORE",
    Label: "trialsStore",
    Default: true,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// Middleman's store-backed offers: the two permanent fusion-slot offers and the weekly Aetherdust
// cell rotation. Independent of STORE for the same reason as Lady Luck; on by default since an in-game
// pass of the 1.4.4 vendor request flow.
export const MiddlemanStore = DefineSwitch({
    Env: "MIDDLEMAN_STORE",
    Label: "middlemanStore",
    Default: true,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// POST /progression/:uid (the game server's XP and objective grant): a body identical to the account's
// last applied grant, less than this many seconds after it, is a retry of a request that failed (the game
// server retries up to 5 times). It gets the stored answer and adds nothing. 0 turns the check off. Keep it
// well under 10: the game server sends its queued grants at most once every 10 s (QueuedGrantTimeout), so
// a window of 10 or more could take a real grant for a retry.
export const ProgressionReplayWindow = DefineSwitch({
    Env: "PROGRESSION_REPLAY_WINDOW_S",
    Label: "replayWindow",
    Default: 5,
    Parse: ParseCount,
    Show: (Value: number) => Value === 0 ? "off" : `${Value}s`
});

// A Hunt Pass or mastery confirm that raises a rank also grants that rank's permanent entitlements from the
// progression config (never items, currencies or timed boosts: the game server pays those through
// /inventory). Off by default: whether the game server grants them itself is open (the in-game test of the
// Elite ranks 6, 9, 29 and 50 decides).
export const ProgressionConfirmEntitlements = DefineSwitch({
    Env: "PROGRESSION_CONFIRM_ENTITLEMENTS",
    Label: "confirmEntitlements",
    Default: false,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// GET /balance and POST /reconcile report the currencies (CURRENCY_*) the account's active character
// holds as inventory stacks, over the fixed sheet (0, weapon tokens 25, Notes from users.notes) for every
// currency it holds. 0 answers the fixed sheet alone, as before.
export const BalanceFromInventory = DefineSwitch({
    Env: "BALANCE_FROM_INVENTORY",
    Label: "balanceFromInventory",
    Default: true,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// Middleman fusion completion checks. Stable per-slot fusion-token persistence is always enabled because
// it only disambiguates inventory items the client already sends. The stricter reveal-time/result validation
// waits for a captured 1.4.4 completion transaction so a guessed request shape cannot block legitimate saves.
export const MiddlemanFusionGuards = DefineSwitch({
    Env: "MIDDLEMAN_FUSION_GUARDS",
    Label: "middlemanFusionGuards",
    Default: false,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// Trials leaderboards (roadmap 3.8; controllers/trials.ts). On by default; TRIALS_LEADERBOARDS=off
// turns them off. When on, game servers may submit completed runs to the aggregate endpoint and
// players may read the five leaderboard routes.
export const TrialsLeaderboards = DefineSwitch({
    Env: "TRIALS_LEADERBOARDS",
    Label: "trialsLeaderboards",
    Default: true,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// Experimental replacement for the expired client-side Trials schedule. When enabled the tuning
// endpoint advertises the Normal/Dauntless player hunts and this week's exact cooked matchmaker rows
// as active for the current Thursday-to-Thursday window. Off by default until a real 1.4.4 client
// proves this is sufficient to remove the Arena branch of the DLL's IsHuntUnlocked bypass.
export const TrialsSchedule = DefineSwitch({
    Env: "TRIALS_SCHEDULE",
    Label: "trialsSchedule",
    Default: false,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// Slayer Links (controllers/slayerlinks.ts): two friends link up for a week. On by default: the routes
// only answer what 404'd before. 0 puts the 404s back (the stored invites and links stay).
export const SlayerLinks = DefineSwitch({
    Env: "SLAYER_LINKS",
    Label: "slayerLinks",
    Default: true,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// Friends' online status in chat (realtime/presence.ts; needs CHAT=1): the chat server relays each
// player's own presence to their online friends and tells them when the player goes offline. On by
// default; CHAT_PRESENCE=0 is an operator rollback switch. Self-presence guards remain enforced
// (docs/findings/chat.md, party safety). Read when the chat server starts.
export const ChatPresence = DefineSwitch({
    Env: "CHAT_PRESENCE",
    Label: "chatPresence",
    Default: true,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// GET /account/api/oauth/verify (the client's check every 30 s) names the account of the token it was
// sent with, as the real service did. 1 answers the old fixed placeholder account to everyone.
export const VerifyStubAccount = DefineSwitch({
    Env: "VERIFY_STUB_ACCOUNT",
    Label: "verifyStubAccount",
    Default: false,
    Parse: ParseOnOff,
    Show: ShowOnOff
});

// One line for the boot log, e.g. "features: bodyLogPerPath=no-cap"
export function DescribeFeatures(){
    return `features: ${Switches.map((Switch) => `${Switch.Label}=${Switch.Show(ReadSwitch(Switch))}`).join(" ")}`;
}
