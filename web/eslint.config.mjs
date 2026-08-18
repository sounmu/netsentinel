import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";
import noStaticStyleProps from "./eslint-rules/no-static-style-props.mjs";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // Pin the hooks rules we rely on as errors. `eslint-config-next@16.x` is a
  // moving target and has historically downgraded `react-hooks/*` to warnings
  // across minor versions; set-state-in-effect especially matters after the
  // React 19 upgrade — regressions in `alerts/page.tsx` / Auth/Theme/I18n
  // contexts already required targeted suppressions, and a drift to "warning"
  // would let silent new violations through.
  {
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      "react-hooks/set-state-in-effect": "error",
    },
  },
  // Design-system enforcement (DESIGN.md §8). Typography, spacing and shape
  // must come from token-backed classes; runtime-computed geometry and colour
  // stay legal, so the rule targets properties rather than banning `style`.
  {
    files: ["app/**/*.tsx"],
    plugins: { netsentinel: { rules: { "no-static-style-props": noStaticStyleProps } } },
    rules: { "netsentinel/no-static-style-props": "error" },
  },
  {
    // Replaces the root layout, so globals.css never loads and no token
    // exists — literal values are correct here.
    files: ["app/global-error.tsx"],
    rules: { "netsentinel/no-static-style-props": "off" },
  },

  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
  ]),
]);

export default eslintConfig;
