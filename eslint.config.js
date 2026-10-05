import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

export default tseslint.config(
  // sdk/ and examples/ hold AssemblyScript, which has its own compiler and dialect.
  { ignores: ["dist", "src-tauri", "node_modules", "sdk", "examples"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: { globals: globals.browser },
    plugins: { "react-hooks": reactHooks },
    rules: { ...reactHooks.configs.recommended.rules },
  },
  // R20: the shared layers stay below the features (docs/architecture/FRONTEND_MAP.md
  // section 1). The architecture check holds the same rules and the feature-to-feature
  // edges; ESLint reports the layer rules in the editor too.
  {
    files: ["src/lib/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: ["react", "react-dom"].map((name) => ({ name, message: "src/lib is plain TypeScript plus IPC, without React (R20)." })),
          patterns: [
            { group: ["react/*", "react-dom/*"], message: "src/lib is plain TypeScript plus IPC, without React (R20)." },
            { group: ["**/features/**", "**/components/**", "**/stores/**"], message: "src/lib imports nothing outside src/lib (R20)." },
          ],
        },
      ],
    },
  },
  {
    files: ["src/components/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": ["error", { patterns: [{ group: ["**/features/**"], message: "src/components may not import a feature (R20)." }] }],
    },
  },
  {
    files: ["src/stores/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        "error",
        { patterns: [{ group: ["**/features/**", "**/components/**"], message: "src/stores holds UI state and imports only src/lib (R20)." }] },
      ],
    },
  },
  // The repository's Node scripts (quality checks and their tests).
  {
    files: ["scripts/**/*.mjs"],
    languageOptions: { globals: globals.node },
  },
);
