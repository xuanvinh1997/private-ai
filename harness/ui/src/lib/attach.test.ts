import { describe, expect, it } from "vitest";
import { type Attached, fileName, withAttachments } from "./attach";

const file = (path: string, extracted = false): Attached => ({
  path,
  name: fileName(path),
  extracted,
});

// The heading is translated; these run in the default locale, since switching one needs a DOM this suite has not.
describe("attachment chips become one message", () => {
  it("keeps the words and lists the files under them", () => {
    const message = withAttachments("  doc giup toi  ", [
      file("/nha/du-an/ghi-chu.md"),
      file("/nha/du-lieu/dinh-kem/p1/bao-cao.pdf", true),
    ]);

    expect(message).toBe(
      [
        "doc giup toi",
        "",
        "Attached files:",
        "- /nha/du-an/ghi-chu.md (open with read)",
        "- /nha/du-lieu/dinh-kem/p1/bao-cao.pdf (extracted, open with attachment.read)",
      ].join("\n"),
    );
  });

  it("sends the files alone when nothing was typed, since the chips already say `read this`", () => {
    expect(withAttachments("   ", [file("/nha/du-an/a.rs")])).toBe(
      "Attached files:\n- /nha/du-an/a.rs (open with read)",
    );
  });

  it("leaves a plain message untouched, so no attachment means no heading", () => {
    expect(withAttachments(" chao ", [])).toBe("chao");
    expect(withAttachments("   ", [])).toBe("");
  });

  it("names a file by its last segment on either platform", () => {
    expect(fileName("/nha/du-an/bao-cao.pdf")).toBe("bao-cao.pdf");
    expect(fileName("C:\\Users\\vinh\\bao cao.docx")).toBe("bao cao.docx");
    expect(fileName("mot-minh.txt")).toBe("mot-minh.txt");
  });
});
