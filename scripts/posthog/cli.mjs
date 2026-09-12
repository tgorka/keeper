import { run, safeMessage } from "./provision.mjs";

try {
  await run(process.argv.slice(2), process.env);
} catch (error) {
  console.error(safeMessage(error));
  process.exitCode = 1;
}
