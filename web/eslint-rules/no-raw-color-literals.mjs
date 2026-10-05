/**
 * Disallows raw CSS colour literals in TSX source.
 *
 * CSS variables are the only way a colour can follow both application
 * themes. Stylelint covers CSS files; this rule closes the equivalent gap in
 * component constants, JSX props and inline styles.
 */
const RAW_COLOR = /(?:#[\da-f]{3,8}\b|\b(?:rgb|hsl)a?\s*\()/i;

function reportIfRawColor(context, node, value) {
  if (RAW_COLOR.test(value)) {
    context.report({ node, messageId: "rawColor" });
  }
}

const rule = {
  meta: {
    type: "problem",
    docs: {
      description: "Disallow raw hex, rgb() and hsl() colour literals in TSX; use a design token.",
    },
    schema: [],
    messages: {
      rawColor: "Raw colour literals must be defined as CSS custom properties and referenced with `var(--token)`.",
    },
  },
  create(context) {
    return {
      Literal(node) {
        if (typeof node.value === "string") {
          reportIfRawColor(context, node, node.value);
        }
      },
      TemplateLiteral(node) {
        for (const quasi of node.quasis) {
          reportIfRawColor(context, quasi, quasi.value.raw);
        }
      },
    };
  },
};

export default rule;
