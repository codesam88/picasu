import type { Plugin } from "@opencode-ai/plugin";

export const AutoCheckPlugin: Plugin = async ({ $ }) => {
  return {
    // Hook name in the JSON config is "PostToolUse"
    "tool.execute.after": async (input, output) => {
      // Trigger condition: only run after file-modifying tools
      const modificationTools = ["write", "edit", "replace", "apply_diff"];

      const isModification = modificationTools.some((t) =>
        input.tool.includes(t)
      );

      if (isModification) {
        console.log(
          `[AutoCheck] Detected file modification by tool: ${input.tool}`
        );

        try {
          // Use the project's justfile commands for validation
          await $`if [ -f gallery-backend/Cargo.toml ]; then just backend-check; fi && if [ -f gallery-frontend/package.json ]; then just frontend-check; fi`;

          console.log("[AutoCheck] All checks passed successfully.");
        } catch (e) {
          // Failure logs to console; the AI sees the error and can fix it
          console.error("[AutoCheck] Check failed:", e);

          // Uncomment to surface the error to the AI as a tool failure:
          // throw new Error(`Auto-check failed after edit: ${e.message}`);
        }
      }
    },
  };
};
