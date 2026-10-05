import { describe, expect, it } from "vitest";
import { cargoInvocation } from "./cargo.mjs";
import { stepsFor } from "./check.mjs";
import { anchorsOf, brokenLinks, linksOf, slugify } from "./check-docs-links.mjs";
import { cargoPackageVersion, versionProblems } from "./check-versions.mjs";

describe("versions", () => {
  const cargo = (version) => `[package]\nname = "launchdeck"\nversion = "${version}"\n\n[dependencies]\nserde = { version = "1" }\n`;

  it("reads the [package] version of Cargo.toml only", () => {
    expect(cargoPackageVersion(cargo("0.2.0"))).toBe("0.2.0");
    expect(cargoPackageVersion('[dependencies]\nversion = "9.9.9"\n[package]\nname = "x"\n')).toBeNull();
  });

  it("passes equal versions and an absent tauri.conf.json version", () => {
    expect(versionProblems({ packageVersion: "0.2.0", cargoVersion: "0.2.0", tauriVersion: "0.2.0" })).toEqual([]);
    expect(versionProblems({ packageVersion: "0.3.0", cargoVersion: "0.3.0", tauriVersion: undefined })).toEqual([]);
  });

  it("names each version that differs or is missing", () => {
    const cargoOff = versionProblems({ packageVersion: "0.3.0", cargoVersion: "0.2.0", tauriVersion: undefined });
    expect(cargoOff).toHaveLength(1);
    expect(cargoOff[0]).toContain("src-tauri/Cargo.toml");
    expect(cargoOff[0]).toContain("0.2.0");

    const tauriOff = versionProblems({ packageVersion: "0.3.0", cargoVersion: "0.3.0", tauriVersion: "0.2.0" });
    expect(tauriOff).toHaveLength(1);
    expect(tauriOff[0]).toContain("tauri.conf.json");

    expect(versionProblems({ packageVersion: undefined, cargoVersion: "0.3.0" })).toHaveLength(1);
  });
});

describe("doc links", () => {
  it("slugs headings the way GitHub does", () => {
    expect(slugify("Regenerating a contract fixture")).toBe("regenerating-a-contract-fixture");
    expect(slugify("5. Import matrix and allowlists")).toBe("5-import-matrix-and-allowlists");
    expect(slugify("`check:arch` and **R1**: why?")).toBe("checkarch-and-r1-why");
    expect(slugify("Rings & dependency direction")).toBe("rings--dependency-direction");
    expect(slugify("snake_case stays")).toBe("snake_case-stays");
  });

  it("collects headings, numbering duplicates, and skips code and front matter", () => {
    const md = "---\nname: x\n---\n# Title\n## Setup\n```\n# not a heading\n```\n## Setup\nSetext one\n----------\n";
    expect([...anchorsOf(md)]).toEqual(["title", "setup", "setup-1", "setext-one"]);
  });

  it("finds links outside code only", () => {
    const md = [
      "See [the map](docs/map.md#rings) and ![logo](img/logo.png 'Logo').",
      "`[not a link](nowhere.md)` and [web](https://example.com).",
      "```md",
      "[also not](nowhere.md)",
      "```",
      "[ref]: <docs/other file.md>",
    ].join("\n");
    expect(linksOf(md)).toEqual([
      { target: "docs/map.md#rings", line: 1 },
      { target: "img/logo.png", line: 1 },
      { target: "https://example.com", line: 2 },
      { target: "docs/other file.md", line: 6 },
    ]);
  });

  const files = {
    "README.md": "# Readme\n[ok](docs/a.md) [dir](docs) [top](#readme) [a](docs/a.md#part-one) [web](https://x.y/z.md)\n",
    "docs/a.md": "# A\n## Part one\n[back](../README.md#readme) [self](#part-one) [up](../src/main.ts#L3)\n",
    "src/main.ts": "",
  };
  const io = (extra = {}) => {
    const all = { ...files, ...extra };
    return {
      read: (file) => all[file] ?? null,
      exists: (p) => p in all || Object.keys(all).some((f) => f.startsWith(`${p}/`)),
    };
  };

  it("passes links that resolve", () => {
    expect(brokenLinks(Object.keys(files).filter((f) => f.endsWith(".md")), io())).toEqual([]);
  });

  it("finds a broken relative link and an anchor mismatch", () => {
    const extra = { "docs/b.md": "# B\n[gone](missing.md)\n[bad anchor](a.md#part-two)\n[here](#nope)\n" };
    const broken = brokenLinks(["docs/b.md"], io(extra));
    expect(broken.map((b) => [b.line, b.target])).toEqual([
      [2, "missing.md"],
      [3, "a.md#part-two"],
      [4, "#nope"],
    ]);
    expect(broken[0].reason).toContain("docs/missing.md");
    expect(broken[1].reason).toContain("part-two");
  });
});

describe("cargo wrapper", () => {
  it("puts the manifest path right after the subcommand and passes the rest through", () => {
    expect(cargoInvocation(["clippy", "--all-targets", "--", "-D", "warnings"], {}, "/repo")).toEqual({
      command: "cargo",
      args: ["clippy", "--manifest-path", "/repo/src-tauri/Cargo.toml", "--all-targets", "--", "-D", "warnings"],
    });
    expect(cargoInvocation(["run", "--example", "x", "--", "a"], { LAUNCHDECK_CARGO: "/tmp/cargo.sh" }, "/repo")).toEqual({
      command: "/tmp/cargo.sh",
      args: ["run", "--manifest-path", "/repo/src-tauri/Cargo.toml", "--example", "x", "--", "a"],
    });
    expect(cargoInvocation(["+1.97.0", "fmt", "--check"], {}, "/repo").args).toEqual([
      "+1.97.0",
      "fmt",
      "--manifest-path",
      "/repo/src-tauri/Cargo.toml",
      "--check",
    ]);
    expect(() => cargoInvocation([], {}, "/repo")).toThrow(/usage/);
  });
});

describe("check runner", () => {
  const names = (mode) => stepsFor(mode).map((s) => s.name);

  it("lists the steps of each mode in order", () => {
    const fast = ["typecheck", "lint", "architecture", "versions", "doc links", "vitest", "cargo fmt", "cargo clippy"];
    expect(names("check")).toEqual(fast);
    expect(names("web")).toEqual(fast.slice(0, 6));
    expect(names("full")).toEqual([...fast, "cargo test", "vite build"]);
  });

  it("limits the architecture check to the web rules in web mode", () => {
    const web = stepsFor("web").find((s) => s.name === "architecture");
    expect(web.args).toContain("--only=web");
    expect(stepsFor("check").find((s) => s.name === "architecture").args).not.toContain("--only=web");
  });
});
