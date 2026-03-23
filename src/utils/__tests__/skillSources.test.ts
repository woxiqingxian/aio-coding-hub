import { describe, expect, it } from "vitest";
import { displaySkillName, repoPrefixFromGitUrl } from "../skillSources";

describe("utils/skillSources", () => {
  it("uses owner and repo as the displayed repo prefix when possible", () => {
    expect(repoPrefixFromGitUrl("https://github.com/acme/repo-one.git")).toBe("acme/repo-one");
    expect(displaySkillName("Alpha", "https://github.com/acme/repo-one.git")).toBe(
      "acme/repo-one/Alpha"
    );
  });

  it("falls back to the last segment when the path has no owner prefix", () => {
    expect(repoPrefixFromGitUrl("repo-one")).toBe("repo-one");
    expect(displaySkillName("Alpha", "repo-one")).toBe("repo-one/Alpha");
  });
});
