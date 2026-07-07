import { classifyLicense, readLicenseField } from "./check-js-licenses";

describe("classifyLicense", () => {
  describe("permissive licenses classify as allow", () => {
    const permissive = ["MIT", "Apache-2.0", "BSD-3-Clause", "ISC", "MPL-2.0"];
    for (const spdx of permissive) {
      it(`allows ${spdx}`, () => {
        expect(classifyLicense(spdx)).toBe("allow");
      });
    }
  });

  describe("copyleft licenses classify as deny", () => {
    const copyleft = ["GPL-3.0", "GPL-2.0", "AGPL-3.0", "LGPL-3.0", "SSPL-1.0"];
    for (const spdx of copyleft) {
      it(`denies ${spdx}`, () => {
        expect(classifyLicense(spdx)).toBe("deny");
      });
    }

    it("denies modern `-only`/`-or-later` copyleft ids", () => {
      expect(classifyLicense("GPL-3.0-or-later")).toBe("deny");
      expect(classifyLicense("LGPL-2.1-only")).toBe("deny");
    });
  });

  describe("SPDX expressions", () => {
    it("denies when any token is copyleft (deny wins on mixed)", () => {
      expect(classifyLicense("(MIT OR GPL-2.0-only)")).toBe("deny");
    });

    it("allows when all tokens are permissive", () => {
      expect(classifyLicense("Apache-2.0 AND MIT")).toBe("allow");
    });

    it("allows a permissive license carrying a WITH exception", () => {
      expect(classifyLicense("Apache-2.0 WITH LLVM-exception")).toBe("allow");
    });

    it("still denies a copyleft license carrying a WITH exception", () => {
      expect(classifyLicense("GPL-2.0-only WITH Classpath-exception-2.0")).toBe("deny");
    });

    it("allows a permissive license with an SPDX `+` (or-later) suffix", () => {
      expect(classifyLicense("Apache-2.0+")).toBe("allow");
    });
  });

  describe("unknown / missing licenses classify as unknown", () => {
    it("treats an unrecognized string as unknown", () => {
      expect(classifyLicense("SEE LICENSE IN LICENSE.txt")).toBe("unknown");
    });

    it("treats an empty string as unknown", () => {
      expect(classifyLicense("")).toBe("unknown");
    });
  });
});

describe("readLicenseField", () => {
  it("reads the modern string form", () => {
    expect(readLicenseField({ license: "MIT" })).toBe("MIT");
  });

  it("reads the deprecated object form so copyleft cannot hide in it", () => {
    expect(readLicenseField({ license: { type: "GPL-3.0" } })).toBe("GPL-3.0");
  });

  it("reads the deprecated `licenses` array form", () => {
    expect(readLicenseField({ licenses: [{ type: "MIT" }, { type: "Apache-2.0" }] })).toBe(
      "MIT OR Apache-2.0",
    );
  });

  it("returns null when no license field is present", () => {
    expect(readLicenseField({ name: "x", version: "1.0.0" })).toBeNull();
  });
});
