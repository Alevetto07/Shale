import { describe, expect, it } from "vitest";
import { parseImportManifest } from "./slicer-import-manifest";

describe("slicer import manifest", () => {
  it("reads string and object preset names", () => {
    const text = `{"presets":["a",{"name":"b"}]}`;
    expect(parseImportManifest(text)).toEqual(["a", "b"]);
  });

  it("rejects invalid manifests", () => {
    expect(() => parseImportManifest("{}")).toThrow(/presets array/);
  });
});