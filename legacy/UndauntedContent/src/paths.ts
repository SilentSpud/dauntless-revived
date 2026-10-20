// Turns the raw request target of GET /content/v1/files/<path> into a manifest key.
//
// Nothing clever happens here on purpose. The caller looks the result up in an exact-match Map built
// from the manifest and opens the manifest's own path, so a request can only ever name a file the
// manifest lists. This function just refuses the shapes that have no business in a request at all,
// so they are answered with 400 and logged instead of quietly missing the Map.

export const FILES_PREFIX = "/content/v1/files/";

export type FilePathResult =
    | { ok: true; path: string }
    | { ok: false; reason: string };

// Percent-encoded "/", "\", NUL and "." ("%2e" is how "../" usually gets smuggled past naive checks).
const ENCODED_FORBIDDEN = /%(2f|5c|00|2e)/i;

// Anything below 0x20, DEL, and characters no manifest path contains but that mean something to Windows.
const FORBIDDEN_CHARS = /[\x00-\x1f\x7f\\:*?"<>|]/;

// RawTarget is req.url as received (path plus optional query), not yet decoded.
export function ParseFileRequestPath(RawTarget: string): FilePathResult {
    const QueryAt = RawTarget.indexOf("?");
    const RawPath = QueryAt === -1 ? RawTarget : RawTarget.slice(0, QueryAt);

    if(!RawPath.startsWith(FILES_PREFIX)){
        return { ok: false, reason: "prefix" };
    }

    const Rest = RawPath.slice(FILES_PREFIX.length);

    if(Rest.length === 0){
        return { ok: false, reason: "empty" };
    }

    if(Rest.length > 1024){
        return { ok: false, reason: "too_long" };
    }

    if(Rest.includes("\\") || Rest.includes("\0")){
        return { ok: false, reason: "raw_forbidden_char" };
    }

    if(ENCODED_FORBIDDEN.test(Rest)){
        return { ok: false, reason: "encoded_separator" };
    }

    let Decoded: string;
    try{
        Decoded = decodeURIComponent(Rest);
    }
    catch{
        return { ok: false, reason: "bad_encoding" };
    }

    if(FORBIDDEN_CHARS.test(Decoded)){
        return { ok: false, reason: "forbidden_char" };
    }

    // Still percent signs after one decode means double encoding. No manifest path has one.
    if(Decoded.includes("%")){
        return { ok: false, reason: "double_encoding" };
    }

    const Segments = Decoded.split("/");
    for(const Segment of Segments){
        if(Segment === "" || Segment === "." || Segment === ".."){
            return { ok: false, reason: "bad_segment" };
        }
    }

    return { ok: true, path: Decoded };
}
