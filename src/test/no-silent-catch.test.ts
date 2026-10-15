/**
 * Repo-wide guard (Epic 100, story 100.1 #6, R381): a failed call never ends
 * in silence. An error reaches a person and the log, never only one of them —
 * so a catch that swallows a call's failure without a word is a defect, and
 * this test fails on one anywhere in `src/` outside `components/ui/`.
 *
 * What counts as swallowing, read from the TypeScript syntax tree (not text,
 * so formatting cannot hide one):
 *  - a promise `.catch(handler)` whose handler does nothing — an empty body,
 *    or one that only yields `undefined`, `null` or `void 0`. A promise's
 *    failure is a call's failure: every one of these was an IPC call.
 *  - a `try` whose block awaits something, with a `catch` whose block holds
 *    no statement. A comment explains a swallow; it does not report it.
 *
 * The fix is `logFailure(source)` (nobody can feel it) or
 * `sayFailure(source, fallback)` (somebody can) from `@/lib/failure`, or a
 * handler that does something with the failure.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const SRC = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Generated code, shadcn's, and tests are not the app's handling of failures. */
function scanned(path: string): boolean {
  const rel = relative(SRC, path).split("\\").join("/");
  return (
    /\.(ts|tsx)$/.test(rel) &&
    !/\.test\.(ts|tsx)$/.test(rel) &&
    !rel.startsWith("components/ui/") &&
    !rel.startsWith("test/") &&
    !rel.startsWith("lib/ipc/gen/") &&
    !rel.endsWith(".d.ts")
  );
}

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? sources(path) : scanned(path) ? [path] : [];
  });
}

/** Whether an expression is "nothing": `undefined`, `null` or `void <literal>`. */
function isNothing(node: ts.Expression): boolean {
  const inner = ts.isParenthesizedExpression(node) ? node.expression : node;
  return (
    (ts.isIdentifier(inner) && inner.text === "undefined") ||
    inner.kind === ts.SyntaxKind.NullKeyword ||
    (ts.isVoidExpression(inner) && ts.isLiteralExpression(inner.expression))
  );
}

/** Whether a promise handler does nothing with the failure. */
function silentHandler(node: ts.Expression): boolean {
  if (!ts.isArrowFunction(node) && !ts.isFunctionExpression(node)) {
    return false;
  }
  const body = node.body;
  return ts.isBlock(body) ? body.statements.length === 0 : isNothing(body);
}

function awaits(node: ts.Node): boolean {
  let found = false;
  const visit = (child: ts.Node) => {
    if (found || ts.isFunctionLike(child)) {
      return;
    }
    if (ts.isAwaitExpression(child)) {
      found = true;
      return;
    }
    ts.forEachChild(child, visit);
  };
  ts.forEachChild(node, visit);
  return found;
}

/** `file:line` of every silent catch in `text`. */
function silentCatches(file: string, text: string): string[] {
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const found: string[] = [];
  const at = (node: ts.Node) =>
    `${file}:${source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1}`;
  const visit = (node: ts.Node) => {
    if (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      node.expression.name.text === "catch" &&
      node.arguments.length === 1 &&
      silentHandler(node.arguments[0])
    ) {
      found.push(at(node.expression.name));
    }
    if (
      ts.isTryStatement(node) &&
      node.catchClause !== undefined &&
      node.catchClause.block.statements.length === 0 &&
      awaits(node.tryBlock)
    ) {
      found.push(at(node.catchClause));
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return found;
}

describe("no silent catch (R381)", () => {
  it("finds each shape it forbids and none it allows", () => {
    const text = [
      "void a().catch(() => {});",
      "void b().catch(() => undefined);",
      "void c().catch(() => null);",
      "void d().catch(function () {});",
      "async function e() { try { await f(); } catch { /* why */ } }",
      "void g().catch((error) => report(error));",
      "async function h() { try { await i(); } catch { report(); } }",
      "function j() { try { JSON.parse(k); } catch {} }",
    ].join("\n");
    expect(silentCatches("x.ts", text)).toEqual(["x.ts:1", "x.ts:2", "x.ts:3", "x.ts:4", "x.ts:5"]);
  });

  it("holds across src/ outside components/ui", () => {
    const found = sources(SRC).flatMap((path) =>
      silentCatches(relative(SRC, path), readFileSync(path, "utf8")),
    );
    expect(found).toEqual([]);
  });
});
