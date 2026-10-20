import { logger } from "../logger";

// Which accounts get real progression. Real is the default: with PROGRESSION_MODE
// unset, empty or "real", every account stores and reads its own progression.
// PROGRESSION_MODE=stub keeps upstream's stubs (fake max ranks, nothing stored) for
// everyone except the accounts listed in PROGRESSION_REAL_ACCOUNTS (comma-separated
// account ids, e.g. UID-...); that list only matters in stub mode. Any other value
// (off, 0, false...) is logged and treated as real; before real became the default,
// such values meant stub. Real mode covers progression and objectives, Hunt
// Pass, entitlements, loadout slots, cooldowns and bounties together; mixing a fake
// GET with a real grant makes ranks jump backwards. Both variables are read from the
// environment, so a change needs a restart.

type ModeSettings = {
    ModeRaw: string | undefined,
    AccountsRaw: string | undefined,
    Global: boolean,
    Default: boolean,
    Unrecognised: boolean,
    Accounts: Set<string>
};

let Settings: ModeSettings | undefined;

function GetSettings(){
    const ModeRaw = process.env.PROGRESSION_MODE;
    const AccountsRaw = process.env.PROGRESSION_REAL_ACCOUNTS;

    if(Settings != undefined && Settings.ModeRaw === ModeRaw && Settings.AccountsRaw === AccountsRaw){
        return Settings;
    }

    const Mode = (ModeRaw ?? "").trim().toLowerCase();
    const Known = Mode === "" || Mode === "stub" || Mode === "real";

    if(!Known){
        logger.warn(`PROGRESSION_MODE=${ModeRaw} is not "real" or "stub"; using real (the default). Set PROGRESSION_MODE=stub for upstream's fake max ranks`);
    }

    Settings = {
        ModeRaw: ModeRaw,
        AccountsRaw: AccountsRaw,
        Global: Mode !== "stub",
        Default: Mode === "",
        Unrecognised: !Known,
        Accounts: new Set((AccountsRaw ?? "").split(",").map((Id) => Id.trim()).filter((Id) => Id.length > 0))
    };

    return Settings;
}

export function IsRealProgressionAccount(AccountId: unknown){
    if(typeof AccountId !== "string" || AccountId.length === 0){
        return false;
    }

    const Current = GetSettings();

    return Current.Global || Current.Accounts.has(AccountId);
}

// For the startup log line "Progression mode: ...". The stub wording is unchanged from
// when stub was the default (scripts compare it).
export function DescribeProgressionMode(){
    const Current = GetSettings();

    if(!Current.Global){
        return `stub, real for ${Current.Accounts.size} listed account(s)`;
    }

    const Ignored = Current.Accounts.size > 0 ? "; PROGRESSION_REAL_ACCOUNTS is ignored outside stub mode" : "";

    const Why = Current.Default ? " (the default)" : Current.Unrecognised ? ` (PROGRESSION_MODE=${Current.ModeRaw} is not recognised)` : "";

    return `real for every account${Why}${Ignored}`;
}

export function IsProgressionModeStub(){
    return !GetSettings().Global;
}
