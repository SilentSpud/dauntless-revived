// Launcher version rules, shared by tools/ci/check-repo.ts and .github/workflows/launcher-release.yml.
//
// A launcher version is a semantic version (semver.org) without build metadata: it names the release tag
// launcher-v<version>, and Squirrel, which updates installed launchers, drops build metadata, so 0.2.0+a and
// 0.2.0+b would be the same package to it. No dependencies.
//
//   bun tools/ci/launcher-version.ts valid <version>             exit 0 if <version> is a launcher version
//   bun tools/ci/launcher-version.ts compare <a> <b>             prints -1, 0 or 1 (semver precedence)
//   bun tools/ci/launcher-version.ts newest <version> [<other>...]
//       exit 0 if <version> is newer than every other one that counts: a release (no -prerelease part) is
//       compared with the other releases only, a prerelease with everything. Otherwise prints the newer or
//       equal one and exits 1. Others that are not launcher versions are skipped with a warning.
//   bun tools/ci/launcher-version.ts nuget <version>             the version in Squirrel's package names
//   bun tools/ci/launcher-version.ts from-nupkg <file name>      the version in a *-full.nupkg name

const VERSION =
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?$/;

interface Parsed {
    core: [number, number, number];
    pre: string[];
}

function parse(version: unknown): Parsed | null {
    const m = VERSION.exec(String(version));
    if (!m) return null;
    return { core: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] === undefined ? [] : m[4].split(".") };
}

export function isLauncherVersion(version: unknown): version is string {
    return typeof version === "string" && parse(version) !== null;
}

export function isPrerelease(version: string): boolean {
    const v = parse(version);
    return v !== null && v.pre.length > 0;
}

// Semantic Versioning 2.0.0, section 11.
export function compare(a: string, b: string): -1 | 0 | 1 {
    const x = parse(a), y = parse(b);
    if (!x || !y) throw new Error(`not a launcher version: ${!x ? a : b}`);
    for (let i = 0; i < 3; i++) if (x.core[i] !== y.core[i]) return x.core[i]! < y.core[i]! ? -1 : 1;
    if (x.pre.length === 0 || y.pre.length === 0) return x.pre.length === y.pre.length ? 0 : x.pre.length === 0 ? 1 : -1;
    for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
        if (i >= x.pre.length) return -1;
        if (i >= y.pre.length) return 1;
        const p = x.pre[i]!, q = y.pre[i]!;
        const pn = /^\d+$/.test(p), qn = /^\d+$/.test(q);
        if (pn && qn) { if (Number(p) !== Number(q)) return Number(p) < Number(q) ? -1 : 1; }
        else if (pn !== qn) return pn ? -1 : 1;
        else if (p !== q) return p < q ? -1 : 1;
    }
    return 0;
}

// The first other version that <version> does not beat, or null.
export function blockingVersion(version: string, others: string[]): string | null {
    const release = !isPrerelease(version);
    for (const other of others) {
        if (release && isPrerelease(other)) continue;
        if (compare(version, other) <= 0) return other;
    }
    return null;
}

// What electron-winstaller names the package: build metadata dropped, dots removed from the prerelease part.
export function nugetVersion(version: string): string {
    const parts = String(version).split("+")[0]!.split("-");
    const main = parts.shift()!;
    return parts.length > 0 ? `${main}-${parts.join("-").replace(/\./g, "")}` : main;
}

export function versionFromNupkg(name: string): string | null {
    const m = /^.+?-(\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+)?)-full\.nupkg$/.exec(String(name));
    return m ? m[1]! : null;
}

if (import.meta.main) {
    const [command, ...args] = process.argv.slice(2);
    const usage = (): never => {
        console.error("usage: launcher-version.ts valid|compare|newest|nuget|from-nupkg ...");
        process.exit(2);
    };
    const need = (v: string | undefined): string => {
        if (!isLauncherVersion(v)) {
            console.error(`${JSON.stringify(v)} is not a launcher version (x.y.z or x.y.z-prerelease, no +build)`);
            process.exit(2);
        }
        return v;
    };
    switch (command) {
        case "valid":
            if (args.length !== 1) usage();
            process.exitCode = isLauncherVersion(args[0]) ? 0 : 1;
            break;
        case "compare":
            if (args.length !== 2) usage();
            console.log(compare(need(args[0]), need(args[1])));
            break;
        case "newest": {
            if (args.length < 1) usage();
            const others: string[] = [];
            for (const other of args.slice(1)) {
                if (isLauncherVersion(other)) others.push(other);
                else console.error(`skipped ${JSON.stringify(other)}: not a launcher version`);
            }
            const blocking = blockingVersion(need(args[0]), others);
            if (blocking !== null) {
                console.log(blocking);
                process.exitCode = 1;
            }
            break;
        }
        case "nuget":
            if (args.length !== 1) usage();
            console.log(nugetVersion(need(args[0])));
            break;
        case "from-nupkg": {
            if (args.length !== 1) usage();
            const version = versionFromNupkg(args[0]!);
            if (version === null) process.exitCode = 1;
            else console.log(version);
            break;
        }
        default:
            usage();
    }
}
