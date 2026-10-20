// Single-range support for resumable downloads (RFC 9110 section 14).
//
//   no Range header           -> the whole file (200)
//   bytes=START-END           -> START..END, END clamped to the last byte (206)
//   bytes=START-              -> START..last byte (206)
//   bytes=-N                  -> the last N bytes (206)
//   anything else             -> 416 with "Content-Range: bytes */SIZE": several ranges, other units,
//                                START past the end, END before START, bytes=-0, a zero-byte file,
//                                or text that is not a range at all.

export type RangeResult =
    | { kind: "full" }
    | { kind: "partial"; start: number; end: number }
    | { kind: "unsatisfiable" };

const SINGLE = /^bytes=(\d*)-(\d*)$/;

export function ParseRange(Header: string | undefined, Size: number): RangeResult {
    if(Header === undefined){
        return { kind: "full" };
    }

    const Match = SINGLE.exec(Header.trim());
    if(!Match){
        return { kind: "unsatisfiable" };
    }

    const [, StartText, EndText] = Match;

    // 15 digits is already far past any file we serve; longer is garbage.
    if(StartText.length > 15 || EndText.length > 15){
        return { kind: "unsatisfiable" };
    }

    if(StartText === "" && EndText === ""){
        return { kind: "unsatisfiable" };
    }

    if(Size === 0){
        return { kind: "unsatisfiable" };
    }

    if(StartText === ""){
        const Suffix = Number(EndText);
        if(Suffix === 0){
            return { kind: "unsatisfiable" };
        }
        return { kind: "partial", start: Math.max(0, Size - Suffix), end: Size - 1 };
    }

    const Start = Number(StartText);
    if(Start >= Size){
        return { kind: "unsatisfiable" };
    }

    if(EndText === ""){
        return { kind: "partial", start: Start, end: Size - 1 };
    }

    const End = Number(EndText);
    if(End < Start){
        return { kind: "unsatisfiable" };
    }

    return { kind: "partial", start: Start, end: Math.min(End, Size - 1) };
}

// If-Range with our strong ETag: resume only if the file is still the one the client started on.
// A date or a different tag means "send the whole file" (RFC 9110 13.1.5).
export function IfRangeAllowsPartial(IfRange: string | undefined, ETag: string): boolean {
    if(IfRange === undefined){
        return true;
    }

    return IfRange.trim() === ETag;
}

// If-None-Match: "*" or a list of tags, compared weakly as RFC 9110 13.1.2 says (W/"x" matches "x").
export function IfNoneMatchHits(Header: string | undefined, ETag: string): boolean {
    if(Header === undefined){
        return false;
    }

    const Trimmed = Header.trim();
    if(Trimmed === "*"){
        return true;
    }

    return Trimmed.split(",").some((Tag) => Tag.trim().replace(/^W\//, "") === ETag);
}
