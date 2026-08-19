/**
 * Bans design-system properties in JSX `style={{}}` objects.
 *
 * A blanket ban on `style` would be wrong: bar widths, grid spans and
 * threshold colours are genuinely computed at runtime and cannot live in
 * a stylesheet. Ban those and the codebase fills up with
 * `eslint-disable` comments, at which point the rule stops meaning
 * anything.
 *
 * So this bans by PROPERTY instead. Typography, spacing and shape must
 * come from a class backed by the tokens in globals.css; geometry and
 * colour that depend on state stay legal.
 *
 * See DESIGN.md §8.
 */
const BANNED = new Set([
  "fontSize",
  "fontWeight",
  "fontFamily",
  "lineHeight",
  "letterSpacing",
  "padding", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft",
  "margin", "marginTop", "marginRight", "marginBottom", "marginLeft",
  "borderRadius",
  "gap", "rowGap", "columnGap",
  "boxShadow",
]);

/** Literal values are the problem; a computed one may still be legitimate. */
function isStaticValue(node) {
  if (!node) return false;
  if (node.type === "Literal") return true;
  if (node.type === "TemplateLiteral") return node.expressions.length === 0;
  if (node.type === "UnaryExpression") return isStaticValue(node.argument);
  return false;
}

const rule = {
  meta: {
    type: "problem",
    docs: {
      description:
        "Disallow static design-system properties in inline style objects; use a token-backed class instead.",
    },
    schema: [],
    messages: {
      banned:
        "`{{prop}}` must come from a class backed by the design tokens, not an inline style. See DESIGN.md §8.",
    },
  },
  create(context) {
    return {
      JSXAttribute(node) {
        if (node.name?.name !== "style") return;
        const expr = node.value?.expression;
        if (expr?.type !== "ObjectExpression") return;

        for (const prop of expr.properties) {
          if (prop.type !== "Property") continue;
          const name =
            prop.key.type === "Identifier" ? prop.key.name
            : prop.key.type === "Literal" ? String(prop.key.value)
            : null;
          if (!name || !BANNED.has(name)) continue;
          if (!isStaticValue(prop.value)) continue;

          context.report({ node: prop, messageId: "banned", data: { prop: name } });
        }
      },
    };
  },
};

export default rule;
