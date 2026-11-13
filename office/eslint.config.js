import js from "@eslint/js";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

/**
 * Focused typed lint lane.
 *
 * Biome remains the formatter and the fast syntactic linter. This config adds
 * only the rules that require the TypeScript program or that Biome does not
 * cover today: promise safety, React Hooks/Compiler rules and static a11y.
 */
export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "coverage/**",
      "e2e-browser/**",
      "vite.config.ts",
      "vitest.config.ts",
      "vitest.browser.config.ts",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "react-hooks": reactHooks,
      "jsx-a11y": jsxA11y,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      ...jsxA11y.flatConfigs.recommended.rules,
      // Scoped React Compiler migration exception: the current Office views
      // predate these rules. Enabling them now would mix a broad refactor into
      // the harness change, so they stay off until a dedicated migration.
      // rules-of-hooks and exhaustive-deps remain active.
      "react-hooks/set-state-in-effect": "off",
      "react-hooks/preserve-manual-memoization": "off",
      "react-hooks/refs": "off",
      "react-hooks/immutability": "off",
      "@typescript-eslint/no-floating-promises": "error",
      "@typescript-eslint/no-misused-promises": "error",
      "@typescript-eslint/no-explicit-any": "warn",
      "react-hooks/exhaustive-deps": "warn",
    },
  },
  {
    // These surfaces are intentionally keyboard-focusable scroll regions
    // (code/diff overflow) or a quick-action form that owns Escape handling.
    // The rationale is local to the component; do not weaken the rule globally.
    files: [
      "src/components/office/CandidateDiff.tsx",
      "src/components/office/CandidatePullRequest.tsx",
      "src/views/ProjectFiles.tsx",
    ],
    rules: {
      "jsx-a11y/no-noninteractive-tabindex": "off",
    },
  },
  {
    // The quick-action form handles Escape at the form boundary so the whole
    // panel closes from any focused control; autoFocus is deliberate.
    files: ["src/components/office/DecisionCard.tsx"],
    rules: {
      "jsx-a11y/no-noninteractive-element-interactions": "off",
      "jsx-a11y/no-autofocus": "off",
    },
  },
  {
    files: ["src/**/*.test.{ts,tsx}", "src/test/**/*.{ts,tsx}"],
    rules: {
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-non-null-assertion": "off",
    },
  },
);
