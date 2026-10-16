#!/usr/bin/env node
// modeldesk bin shim (platform-binary pattern): resolve this platform's
// native binary from the matching @talha7k/modeldesk-<platform>
// optionalDependency and hand it the argv verbatim. Clear, actionable error
// when the binary is missing — never a raw stack trace.
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";

const SUPPORTED = new Set(["darwin-arm64"]);

const platform = `${process.platform}-${process.arch}`;
const require = createRequire(import.meta.url);

let bin;
try {
  bin = require.resolve(`@talha7k/modeldesk-${platform}/bin/modeldesk`);
} catch (err) {
  console.error(`modeldesk: no native binary for ${platform}.`);
  if (!SUPPORTED.has(platform)) {
    console.error(
      `  ${platform} is not supported yet (supported: ${[...SUPPORTED].sort().join(", ")}).`,
    );
    console.error(`  Linux/Windows follow the provider-agnostic core — track releases.`);
  } else {
    console.error(
      `  Install the platform package: npm install -g @talha7k/modeldesk-${platform}`,
    );
  }
  process.exit(1);
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
process.exit(result.status ?? 1);
