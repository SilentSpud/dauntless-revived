# Player downloads from R2

Upload only the files listed in `legacy/UndauntedContent/data/dauntless-1.4.4.json`, preserving their relative paths under a versioned prefix. Verify local SHA-256 values before uploading and run `rclone check` afterward. Do not upload server kits, configuration, databases or logs. Bucket-scoped R2 credentials may require `--s3-no-check-bucket` with `rclone copyto`; the destination bucket must already exist.

After the custom domain serves the verified files over HTTPS, set the content service environment:

```dotenv
CONTENT_DOWNLOAD_BASE_URL=https://downloads.epcr.lol/dauntless-1.4.4/
```

Restart only the content service. Launcher 0.1.26 and newer read this optional field from the host manifest and fetch game files directly from the CDN. Older launchers continue using the existing authenticated VPS file routes. Remove the variable and restart content to route new download jobs back through the VPS.

The CDN uses normal HTTPS certificate validation and receives no launcher account key. The host connection retains its certificate pin. R2 ETags are not SHA-256 values, so CDN downloads validate the completed file against the launcher's compiled size and SHA-256 instead. Range resumes omit the VPS-specific SHA-256 `If-Range` header. Redirects are not followed. Changing a CDN file cannot change the launcher's accepted game build.

The CDN hosts public base-game files; login and game access still use the game backend. Keep R2 upload credentials outside Git and rotate any exposed credentials. Publishing files does not itself redirect older launchers.
