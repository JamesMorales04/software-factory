import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  projectDisplayName,
  readWorkspaceSelection,
  subscribeWorkspaceSelection,
  writeWorkspaceSelection,
} from "./workspace";

describe("workspace defaults", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("reads empty defaults when nothing has been stored", () => {
    expect(readWorkspaceSelection()).toEqual({
      projectId: "",
      profileId: "",
    });
  });

  it("persists a partial patch and notifies subscribers with the full value", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeWorkspaceSelection(listener);

    writeWorkspaceSelection({ projectId: "project-1" });
    expect(listener).toHaveBeenLastCalledWith({
      projectId: "project-1",
      profileId: "",
    });

    writeWorkspaceSelection({ profileId: "profile-1" });
    expect(listener).toHaveBeenLastCalledWith({
      projectId: "project-1",
      profileId: "profile-1",
    });
    expect(readWorkspaceSelection()).toEqual({
      projectId: "project-1",
      profileId: "profile-1",
    });

    writeWorkspaceSelection({ profileId: "profile-1" });
    expect(listener).toHaveBeenCalledTimes(2);

    unsubscribe();
    writeWorkspaceSelection({ projectId: "project-2" });
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it("shows the human name, then the repository leaf, then the id", () => {
    expect(
      projectDisplayName({
        id: "project-1",
        repository: "acme/widgets",
        name: "Widgets",
      }),
    ).toBe("Widgets");
    expect(
      projectDisplayName({
        id: "project-1",
        repository: "acme/widgets/",
        name: "  ",
      }),
    ).toBe("widgets");
    expect(
      projectDisplayName({
        id: "project-1",
        repository: "",
        name: "",
      }),
    ).toBe("project-1");
  });
});
