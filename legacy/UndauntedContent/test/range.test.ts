import { test } from "node:test";
import assert from "node:assert/strict";
import { IfNoneMatchHits, IfRangeAllowsPartial, ParseRange } from "../src/range";

test("no header means the whole file", () => {
    assert.deepEqual(ParseRange(undefined, 100), { kind: "full" });
});

test("satisfiable single ranges", () => {
    assert.deepEqual(ParseRange("bytes=0-0", 100), { kind: "partial", start: 0, end: 0 });
    assert.deepEqual(ParseRange("bytes=0-1048575", 103673520), { kind: "partial", start: 0, end: 1048575 });
    assert.deepEqual(ParseRange("bytes=10-", 100), { kind: "partial", start: 10, end: 99 });
    assert.deepEqual(ParseRange("bytes=99-", 100), { kind: "partial", start: 99, end: 99 });
    assert.deepEqual(ParseRange("bytes=-10", 100), { kind: "partial", start: 90, end: 99 });
    assert.deepEqual(ParseRange("bytes=-1000", 100), { kind: "partial", start: 0, end: 99 });
    assert.deepEqual(ParseRange("bytes=50-5000", 100), { kind: "partial", start: 50, end: 99 });
    assert.deepEqual(ParseRange(" bytes=1-2 ", 100), { kind: "partial", start: 1, end: 2 });
});

test("bad or unsatisfiable ranges", () => {
    for(const Header of [
        "bytes=100-", "bytes=100-200", "bytes=5-4", "bytes=-0", "bytes=-", "bytes=", "bytes=a-b", "bytes=0-1,5-6",
        "items=0-1", "bytes 0-1", "bytes=-1-2", "bytes=1.5-2", "bytes=0x10-", "bytes=9999999999999999-", "", "garbage",
    ]){
        assert.deepEqual(ParseRange(Header, 100), { kind: "unsatisfiable" }, Header);
    }
});

test("zero-byte files have no satisfiable range", () => {
    assert.deepEqual(ParseRange("bytes=0-", 0), { kind: "unsatisfiable" });
    assert.deepEqual(ParseRange("bytes=-1", 0), { kind: "unsatisfiable" });
    assert.deepEqual(ParseRange(undefined, 0), { kind: "full" });
});

test("If-Range only allows a resume of the same file", () => {
    const Tag = '"abc"';
    assert.equal(IfRangeAllowsPartial(undefined, Tag), true);
    assert.equal(IfRangeAllowsPartial('"abc"', Tag), true);
    assert.equal(IfRangeAllowsPartial('"def"', Tag), false);
    assert.equal(IfRangeAllowsPartial('W/"abc"', Tag), false);
    assert.equal(IfRangeAllowsPartial("Wed, 21 Oct 2015 07:28:00 GMT", Tag), false);
});

test("If-None-Match", () => {
    const Tag = '"abc"';
    assert.equal(IfNoneMatchHits(undefined, Tag), false);
    assert.equal(IfNoneMatchHits("*", Tag), true);
    assert.equal(IfNoneMatchHits('"x", "abc"', Tag), true);
    assert.equal(IfNoneMatchHits('W/"abc"', Tag), true);
    assert.equal(IfNoneMatchHits('"abcd"', Tag), false);
});
