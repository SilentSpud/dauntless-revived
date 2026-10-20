---
title: Verifying game files
parent: Findings
nav_order: 1
description: "How we checked that community-shared Dauntless builds are genuine, complete and clean: Authenticode signatures, Phoenix Labs' install manifest and ClamAV."
lang: en
ref: findings/verification
---

{% assign ci_page = site.pages | where: "path", "findings/client-internals.md" | first %}
{% assign assets_page = site.pages | where: "path", "findings/assets.md" | first %}
{% assign friends_page = site.pages | where: "path", "setup/friends.md" | first %}

# Verifying game files
{: .no_toc }

The official Dauntless servers shut down on 30 May 2025, and the game can no longer be downloaded
from any official source. Every copy we have worked with came from a community-shared archive. Before
running anything from those archives we wanted three answers:

1. **Genuine:** are the binaries exactly what Phoenix Labs shipped?
2. **Complete:** is every file of the build present and intact?
3. **Clean:** is there anything in the archive that should not be there?

This page describes how we answered those questions for two builds. Nothing from either archive was
executed until all checks had passed.

| Build | What we received | Result |
|:------|:-----------------|:-------|
| **2.1.1** (final "Awakening" build, UE5, IoStore). Build 682486, signed 18 Dec 2024 | A RAR5 archive, 13.53 GiB, 833 files, 15.52 GB unpacked | Genuine, complete, clean |
| **1.4.4** (UE4, pak v9). `dauntless_rel-1.4.4_Shipping_2020-10-28`, signed 29 Oct 2020 | `BaseGame144.zip`, 10.48 GB, 410 files (471 zip entries counting 61 directory entries). This is the archive the upstream [Undaunted](https://github.com/SyST3MDeV/Undaunted) launcher downloads | Genuine, complete, clean |

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Ground rules

- **Never run anything from an unverified archive.** That includes installers, launchers and the
  game itself.
- **Do the analysis on Linux.** We used Ubuntu under WSL2. A Windows `.exe` or `.dll` cannot run
  there by accident. This is protection against running something by mistake, not a security
  boundary: WSL2 can reach the Windows drives through `/mnt/c` by default. A dedicated VM isolates
  better.
- **Extracted files lose their execute bit** (`chmod -R a-x+X <dir>`).
- **Check the code first and the bulk later.** For 2.1.1 we extracted only the `.exe` and `.dll`
  files (75 files, about 490 MiB) and verified their signatures. Only then did we unpack the rest.

Tools we used:

| Tool | Version | Used for |
|:-----|:--------|:---------|
| `osslsigncode` | 2.13 (OpenSSL 3.5) | Authenticode verification on Linux |
| ClamAV (`clamscan`, `freshclam`) | 1.5.3, daily signatures 28129 (20 Sep 2026) | Malware scan |
| `unrar` | 7.20 | RAR5 extraction (2.1.1) |
| Python 3 `zipfile`, `hashlib`, `pefile` | Ubuntu packages | Zip extraction, manifest hashing, PE inspection |

---

## Safe extraction

### Zip archives (1.4.4)

We extracted with Python's `zipfile` and did not use a GUI tool. The file names inside an archive
decide where files get written. A crafted name such as `../../somewhere/evil.dll` can write outside the
target folder. This attack is called zip-slip. Our extractor resolves every destination path and
refuses anything that falls outside the target folder:

```python
import zipfile, os, shutil, sys

z = zipfile.ZipFile(sys.argv[1])
out = os.path.realpath(sys.argv[2])
for i in z.infolist():
    dest = os.path.realpath(os.path.join(out, i.filename))
    if not dest.startswith(out + os.sep):          # zip-slip guard
        print("REFUSED path escaping target:", i.filename)
        continue
    if i.is_dir():
        os.makedirs(dest, exist_ok=True)
        continue
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with z.open(i) as src, open(dest, "wb") as dst:
        shutil.copyfileobj(src, dst, 1 << 22)
```

**CRC checks come with it.** Every zip entry stores a CRC-32 of its uncompressed data. When
`zipfile` reads an entry to the end, it compares that CRC and raises `BadZipFile` if they differ. So an
extraction that finishes without an exception has also checked the CRC of every entry. All 410
files of `BaseGame144.zip` extracted cleanly, and no entry was refused.

A CRC only proves that the archive was not corrupted in transit. It says nothing about who made the
files. The later checks answer that.

### RAR5 archives (2.1.1)

- Ubuntu's `7zip` package (7-Zip 26.00, packaged without the non-free RAR decoder) can list a RAR5
  archive but cannot unpack it. Every file we tried failed with `Unsupported Method`. We installed
  `unrar` from Ubuntu's multiverse repository instead.
- We checked the `unrar` version: it is 7.20. CVE-2022-30333 is a path-traversal bug in `unrar` on
  Linux and was fixed in 6.12.
- We read the archive listing before extracting. It contained no absolute paths and no `..`
  components.
- `unrar` checks each file's stored checksum while extracting and exits non-zero on a mismatch. Our
  extraction exited with 0, and all 833 files were present afterwards.

---

## Authenticode signatures with osslsigncode

Phoenix Labs signed its Windows binaries with Authenticode. This is the strongest single check we
have, because it is cryptographic. `osslsigncode` checks Authenticode signatures on Linux:

```bash
osslsigncode verify -CAfile /etc/ssl/certs/ca-certificates.crt -in Dauntless-Win64-Shipping.exe
```

The first two lines to read are these:

```text
Current message digest    : B15C6A3F8900BD367F049260D9E0A005531C54F2C26C668CC16F54A3A67BD0E4
Calculated message digest : B15C6A3F8900BD367F049260D9E0A005531C54F2C26C668CC16F54A3A67BD0E4
```

- **Current** is the digest stored inside the signature. The signer computed it when the file was
  signed.
- **Calculated** is the digest `osslsigncode` computes from the file as it is now.

If they match, the signed parts of the file have not changed by a single byte since signing. If they
differ, the file was modified afterwards. The remaining output says **who** signed the file: the signer
certificate, the chain up to a trusted root, and the timestamp countersignature.

### 2.1.1: every signature verifies

All **75 of 75** binaries verified with `Signature verification: ok`. That is 7 executables and 68
DLLs, with zero failures and zero unsigned files. Details for the main executable:

| Check | Result |
|:------|:-------|
| Digest | Current and calculated digests match |
| Signer | `CN=Phoenix Labs Canada ULC`, Burnaby, British Columbia |
| Issuer | DigiCert Trusted G4 Code Signing RSA4096 SHA384 2021 CA1 |
| Chain | Verified to DigiCert Trusted Root G4 |
| Revocation | CRL fetched from DigiCert; the certificate is not revoked |
| Timestamp | 18 Dec 2024 02:03:21 GMT, countersigned by DigiCert's timestamping CA |

The signing certificate expired in October 2025. That does not matter. The timestamp proves the
signature was made while the certificate was valid, which is how code signing is meant to work.

The 2.1.1 archive contains exactly seven executables: `Dauntless-Win64-Shipping.exe`, `Dauntless.exe`,
`start_protected_game.exe`, `EasyAntiCheat_EOS_Setup.exe`, `CrashReportClient.exe`,
`EpicWebHelper.exe` and `UEPrereqSetup_x64.exe`. All seven are signed by Phoenix Labs. The EAC setup
program matters most here, because it installs a Windows service that runs with system privileges.

### 1.4.4: why "FAILED" can still mean untouched

On the 1.4.4 build, `osslsigncode` reported **FAILED for every one of the 39 binaries**. That
included Microsoft's own `dbghelp.dll` and `d3dcompiler_47.dll`. A file that is really tampered with
fails too, so the failure alone tells us nothing. The full output for the main executable shows what
is actually going on:

```text
Current message digest    : D71ACDA54152B287C361B23B9D60AC45B590982C502D4648EA6600CD618E15AD
Calculated message digest : D71ACDA54152B287C361B23B9D60AC45B590982C502D4648EA6600CD618E15AD
  Subject: CN=Phoenix Labs Canada ULC ...   Issuer: CN=thawte SHA256 Code Signing CA
Countersignatures:
  Timestamp time: Oct 29 04:23:10 2020 GMT      Hash Algorithm: sha1
  Issuer: CN=Symantec Time Stamping Services CA - G2
Timestamp verified using:
  Subject: CN=Symantec Time Stamping Services CA - G2
  Issuer : CN=Thawte Timestamping CA ...
  Error: unable to get local issuer certificate
Timestamp Server Signature verification: failed
Signing certificate chain verified using:
  Subject: CN=thawte SHA256 Code Signing CA
  Issuer : CN=thawte Primary Root CA ...
  Error: unable to get local issuer certificate
Signature verification: failed
```

The digests match. Both errors are **chain** errors, and both have the same cause: a root
certificate that the CA bundle does not contain.

- The 2020 timestamp was issued under `Symantec Time Stamping Services CA - G2` (SHA-1), which
  chains up to the old `Thawte Timestamping CA` root.
- Phoenix's 2019 code-signing certificate was issued by `thawte SHA256 Code Signing CA`, which
  chains up to `thawte Primary Root CA`.

Both roots belong to the retired thawte/Symantec hierarchy. The CA bundle of a current Ubuntu
contains no thawte certificate at all (we checked), so OpenSSL cannot finish either chain and reports
the whole verification as failed. The fault is in our verification environment. It says nothing about
the file.

Because the overall verdict was unusable, we compared the digests of every binary:

```bash
find . -type f \( -iname '*.exe' -o -iname '*.dll' \) | sort | while read -r f; do
  out=$(osslsigncode verify -in "$f" 2>&1)
  cur=$(echo "$out" | grep -m1 'Current message digest'    | awk '{print $NF}')
  cal=$(echo "$out" | grep -m1 'Calculated message digest' | awk '{print $NF}')
  if   [ -z "$cur" ];       then echo "UNSIGNED  $f"
  elif [ "$cur" = "$cal" ]; then echo "intact    $f"
  else                           echo "MODIFIED  $f"; fi
done
```

Result: **39 intact, 0 modified, 0 unsigned.**

| Signer | Files | Timestamps |
|:-------|------:|:-----------|
| Phoenix Labs Canada ULC | 22 | 29 Oct 2020, 04:23:10 to 04:23:19 GMT: one signing run of about nine seconds, the day after the build date in `Version.txt` |
| Microsoft Corporation | 10 | 2009 to 2020, the vendor's own dates for redistributable DLLs |
| EasyAntiCheat Oy | 4 | 7 Sep 2020 |
| Mercer Road Corp (Vivox) | 2 | 8 Jan 2020 |
| Overwolf Ltd | 1 | 15 Aug 2017 |

In 1.4.4, `Dauntless.exe` is the EasyAntiCheat bootstrapper, so EasyAntiCheat Oy signed it, not
Phoenix. We traced the cause of the FAILED verdict only on the main executable. For the other 38
files we checked only that the digests match. Their failures presumably come from similar old chains,
but we have not confirmed that.

How to read `osslsigncode` output:

| Output | Meaning |
|:-------|:--------|
| `Signature verification: ok` | Unmodified and chained to a trusted root |
| FAILED, digests **match**, chain error | Unmodified. The chain fails only in your environment, for example because of a retired root |
| FAILED, digests **differ** | Modified after signing. Do not run it |
| No `Current message digest` line | Unsigned. Not a retail binary unless the vendor shipped it unsigned |

What a digest match does not prove on its own: that the embedded certificate belongs to who it claims
to be. For 1.4.4 we accept it for four reasons. The signer is Phoenix's thawte-issued certificate,
valid from September 2019 to October 2022. On the main executable, the only errors `osslsigncode`
reports are the two missing roots. The 22 Phoenix files form one consistent signing
run, and each third-party file carries its own vendor's signature. And the manifest and hash checks
below agree. We did not try adding the two retired thawte roots to the CA file to get a clean `ok`, so
we cannot say whether that would work.

---

## Completeness: Phoenix's install manifest (1.4.4)

The 1.4.4 build ships `Manifest.bin` and `Manifest.bin.json` in its root folder. This is Phoenix's
own install manifest, not an Unreal or Epic Games Store file. The JSON form is easy to read:

```text
{ "TargetFiles": [ ...406 entries... ], "DeployedPaths": ["**"] }

entry: { "RelativePath": "Archon\\Binaries\\Win64\\dbgcore.dll",
         "FileSize": 166720,
         "IsSelfSource": true,
         "MD5Chunks": "185bfb1bfb9315633cfaa60e2473772745" }
```

`MD5Chunks` is a two-character prefix, always `18`, followed by one 32-hex-digit MD5 for each chunk
of the file. The manifest does not state the chunk size. We tried every power of two from 2^16 to 2^26.
Only **2^24 bytes (16 MiB)** gives, for every one of the 406 files, a hash count equal to
`ceil(FileSize / chunk)`. Hex `0x18` is 24, so the prefix probably encodes that exponent. This is
unconfirmed, but it matches.

The check itself hashes each file in 16 MiB pieces and compares the list:

```python
import json, hashlib, os

d = json.load(open("Manifest.bin.json", encoding="utf-8-sig"))
CHUNK = 1 << 24
for e in d["TargetFiles"]:
    rel = e["RelativePath"].replace(chr(92), "/")          # backslashes -> slashes
    want = e["MD5Chunks"][2:]                              # drop the "18" prefix
    want = [want[i:i+32] for i in range(0, len(want), 32)]
    got = []
    with open(rel, "rb") as f:
        while (b := f.read(CHUNK)):
            got.append(hashlib.md5(b).hexdigest())
    got = got or [hashlib.md5(b"").hexdigest()]
    assert os.path.getsize(rel) == e["FileSize"] and got == want, rel
```

**Result: 406 of 406 files match. None mismatched, none had the wrong size, none were missing.** The
only files on disk that the manifest does not list are `Manifest.bin` and `Manifest.bin.json`
themselves, plus two empty `debug.log` files, one in each `Binaries\Win64` folder.

The manifest travels inside the same archive, so on its own it proves consistency, not origin.
Someone who altered a pak could regenerate the manifest to match. Its value comes from being combined
with the signature check: the code that reads those paks is proven genuine, and the whole install
agrees with Phoenix's own file list.

Two more cross-checks for 1.4.4:

- The SHA-256 of the main executable and of the zip both equal the values pinned in the upstream
  Undaunted launcher (`legacy/UndauntedLauncher/src/main.ts`):

  | File | SHA-256 |
  |:-----|:--------|
  | `Dauntless-Win64-Shipping.exe` (103,673,520 bytes) | `D3D41E614908D2BEFD518B27046D9822D6130EF12BA3504BABBDB786BEF9CFF4` |
  | `BaseGame144.zip` | `556B9A648A5E5E7E11B6F8DD3D80FF8E88FCEB0D3448297AAF47CE7BF756BC6D` |

- `EasyAntiCheat/Certificates/game.cer` is EasyAntiCheat's stock certificate. No extra CA
  certificate was slipped into the install.

## Completeness: Unreal's staging manifests (2.1.1)

The 2.1.1 copy came from an Epic Games Store install. It has no `Manifest.bin.json`, but Unreal's
packaging step leaves three manifests in the root folder:

| Manifest | Lists | Present |
|:---------|:------|:--------|
| `Manifest_NonUFSFiles_Win64.txt` | 352 loose files: binaries, EasyAntiCheat, CEF and other files outside the containers | **352 of 352** |
| `Manifest_DebugFiles_Win64.txt` | 6 debug files | 5. Only `Dauntless-Win64-Shipping.pdb` is missing, as expected for a retail install |
| `Manifest_UFSFiles_Win64.txt` | 152,035 packaged asset paths | These are inside the containers and cannot be checked file by file |

These manifests list paths and timestamps, not hashes. They show that the install is complete, not
that its contents are intact. For the containers we checked structure:

- All 157 `.utoc` files carry the IoStore magic `-==--==--==--==-` and TOC version 5.
- Every `.utoc` has its `.ucas` and every `.ucas` its `.utoc`: 157 each, with no orphans. There are
  also 157 Unreal `.pak` files. 156 of them sit beside the containers, since the `global` container
  has no `.pak`, and the last one is `CrashReportClient.pak`. Every one ends in the pak footer magic
  `0x5A6F12E1`.
- The chunk numbering has no gaps in any of the 18 container groups (`Archon_0..50`,
  `Archon_ArmourC_0..40`, `Archon_UI_0..11` and the rest). A missing container would leave a gap.
- **Correction:** our first footer check flagged 56 `.pak` files as invalid. All 56 sit under
  `Engine/Binaries/ThirdParty/CEF3/.../Resources` and are Chromium resource paks, a different format
  with the same extension. Each has a valid Chromium pak header (version 5). The check was wrong, not
  the files.
- The 187 `.chroma` files (Razer Chroma lighting effects) are listed in the loose-file manifest and
  all share the same header.

Apart from the containers, only eight files on disk appear in none of the manifests, and all eight are
expected: the Epic Games Store install record (`.egstore/<id>.manifest`), three EasyAntiCheat runtime
files written at install time (`base.bin`, `base.cer`, `runtime.conf`), the three manifest files
themselves, and the crash reporter's own `CrashReportClient.pak`. The archive contains no scripts
(`.bat`, `.cmd`, `.ps1`, `.vbs`, `.scr`, `.lnk`, `.hta`). The EasyAntiCheat `Settings.json` points at
the retail executable, `Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe`.

---

## ClamAV, and the 2 GiB trap

An antivirus scan is the weakest of these checks. A clean result only means that no known signature
matched. We ran it anyway, because asset files cannot be signature-verified and a scan covers them.

### The trap

Our first attempt on 2.1.1 scanned the RAR archive directly and "passed":

```text
LibClamAV Warning: Max file-size was set to 4293918720 bytes. Unfortunately, scanning files
greater than 2147483647 bytes (2 GiB - 1) is not supported.
Scanned files: 1
Infected files: 0
Data scanned: 0 B
Data read: 13.53 GiB (ratio 0.00:1)
```

**That result is meaningless.** ClamAV cannot scan any single file larger than 2 GiB − 1. Raising
`--max-filesize` or `--max-scansize` above that only produces the warning. The oversized file is
skipped, and the summary still says `Infected files: 0`. The giveaway is `Data scanned: 0 B`. We
briefly treated this run as a clean result before we noticed.

How to avoid it:

1. **Scan the extracted tree, not the archive.**
2. **Make sure no single extracted file is over 2 GiB.** The largest file in either build is an audio
   pak of about 272 MB.
3. **Keep the limits at 2047M or below**, and read `Data scanned` in the summary, not only
   `Infected files`.
4. Optionally add `--alert-exceeds-max=yes`. ClamAV then reports files it skipped because of a limit,
   so they do not pass silently. We did not use this option in the runs below.

```bash
clamscan -r -i --alert-broken \
  --max-filesize=2047M --max-scansize=2047M --max-files=10000 --max-recursion=16 \
  /root/analysis/full
```

For the 1.4.4 run we passed `4000M`. That printed the same warning but did no harm, because no file
came close to 2 GiB.

### Results

| Build | Files scanned | Data read | Data scanned | Infected | Time |
|:------|--------------:|----------:|-------------:|---------:|-----:|
| 2.1.1, binaries only (first pass) | 75 | 491 MiB | 344 MiB | 0 | 2 m 18 s |
| 2.1.1, full extracted tree | 833 | 14.46 GiB | 30.37 GiB | **0** | 22 m 28 s |
| 1.4.4, binaries and text files | 66 | | 257 MiB | 0 | |
| 1.4.4, full extracted tree | 408 | 10.15 GiB | 21.31 GiB | **0** | 16 m 23 s |

`Data scanned` is larger than `Data read` because ClamAV unpacks compressed content it recognises and
scans the result. For 1.4.4, the gap between 408 scanned files and 410 extracted files is the two
zero-byte `debug.log` files: ClamAV reported each as `Empty file` and did not count it.

---

## The prebuilt Undaunted DLLs

The 1.4.4 setup adds two prebuilt files from the upstream Undaunted repository to the game folder:
`dxgi.dll` and `UndauntedInternalServer.dll`. They are not game files, but they run inside the game
process, so we checked them the same way. Both are unsigned, which is normal for a hobby project.

| Check | `dxgi.dll` | `UndauntedInternalServer.dll` |
|:------|:-----------|:------------------------------|
| Size | 11,264 bytes | 123,392 bytes |
| SHA-256 | `9A431D7B…4B0D1F` | `520EC588…0D0933` |
| Matches the git blob in the repository (not a Git LFS pointer) | Yes | Yes |
| Packing, overlay, TLS callbacks, RWX sections | None | None |
| Imports | Only `GetSystemDirectoryA`, `LoadLibraryA`, `GetProcAddress` and the standard MSVC runtime startup imports | Match `dllmain.cpp`, `Networking.cpp`, MinHook and the C runtime. No networking, registry, process-creation or file-writing imports (its one `freopen_s` use attaches stdout and stdin to its own console) |
| ClamAV | OK | OK |

What `dxgi.dll` does, from a full disassembly of its `DllMain`: it loads the real `dxgi.dll` from
System32, looks up `CreateDXGIFactory`, `CreateDXGIFactory1` and `CreateDXGIFactory2`, and forwards its
three exports to them. It then loads `UndauntedInternalServer.dll` by name. It does nothing else. Those
three exports are exactly the set the game needs: the 1.4.4 executable imports `CreateDXGIFactory` and
`CreateDXGIFactory1`, and the system `d3d11.dll` (on our Windows 10 machine) imports
`CreateDXGIFactory2`.

Every hard-coded game address in the server DLL either appears in its published source or comes from
the offsets of the generated SDK. We found none that the source does not explain. Windows Defender
raised no detections when we copied the files into the game folder.

One open issue: **the source of `dxgi.dll` is not in the Undaunted repository.** Its debug path points
to a separate project whose source we have not found published anywhere. That matters under the AGPL. Replacing it with our
own proxy, and building `UndauntedInternalServer.dll` from source ourselves, are both on the roadmap.
Until then we check both files against the pinned hashes before every copy. The full hashes are in the
[friends' setup guide]({{ friends_page.url | relative_url }}).

---

## Checklist

To repeat this on your own copy of 1.4.4:

1. Hash the archive and compare it with `556B9A64…6BC6D`.
2. Extract it inside a Linux environment with a zip-slip guard, and let the CRC checks run.
3. Run `osslsigncode verify` on every `.exe` and `.dll` and compare the current and calculated
   digests. A FAILED whose only errors are the missing thawte roots is a chain problem, not
   tampering, as long as the digests match.
4. Check every file against `Manifest.bin.json` in 16 MiB MD5 chunks.
5. Scan the extracted tree with ClamAV. Confirm that `Data scanned` is not zero.
6. Only then copy the install to Windows.

For what the verified files contain, see [Game assets and config]({{ assets_page.url | relative_url }}).
For what the executable can and cannot do, see [Client internals]({{ ci_page.url | relative_url }}).
