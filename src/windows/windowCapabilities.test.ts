// @vitest-environment node

import { describe, expect, it } from "vitest";
import capabilityJson from "../../src-tauri/capabilities/default.json";

describe("native window capabilities", () => {
  it("grants the narrow event and geometry commands used by tab dragging", () => {
    const capability = capabilityJson as {
      windows: string[];
      permissions: string[];
    };

    expect(capability.windows).toEqual(["main", "twominal-*"]);
    expect(capability.permissions).toEqual(
      expect.arrayContaining([
        "core:event:allow-emit-to",
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        "core:webview:allow-create-webview-window",
        "core:window:allow-close",
        "core:window:allow-cursor-position",
        "core:window:allow-get-all-windows",
        "core:window:allow-outer-position",
        "core:window:allow-outer-size",
      ]),
    );
    expect(capability.permissions).not.toContain("core:event:default");
  });
});
