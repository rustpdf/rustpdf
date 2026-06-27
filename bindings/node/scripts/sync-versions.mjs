#!/usr/bin/env node
// Single source of truth for the published version: the main package.json.
//
// This stamps every npm/<platform>/package.json with the main package's version
// and rebuilds the main package's `optionalDependencies` map (pinned to the exact
// same version) from the directories under npm/. Run it before publishing so a
// lone `npm version <x>` bump on the main package propagates everywhere — the
// loader resolves `@rustpdf/<platform>@<main version>`, so a platform package at
// a drifting version would be silently skipped at install time and the cdylib
// would go missing. Mirrors how the Python wheels all take their version from one
// pyproject.toml.

import { readFileSync, writeFileSync, readdirSync, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..'); // bindings/node
const npmDir = join(root, 'npm');
const mainPath = join(root, 'package.json');

const readJson = (p) => JSON.parse(readFileSync(p, 'utf8'));
const writeJson = (p, v) => writeFileSync(p, JSON.stringify(v, null, 2) + '\n');

const main = readJson(mainPath);
const { version } = main;

const optional = {};
for (const entry of readdirSync(npmDir).sort()) {
  const pkgDir = join(npmDir, entry);
  if (!statSync(pkgDir).isDirectory()) continue;
  const pkgPath = join(pkgDir, 'package.json');
  const pkg = readJson(pkgPath);
  pkg.version = version;
  writeJson(pkgPath, pkg);
  optional[pkg.name] = version; // exact pin, not ^/~
}

main.optionalDependencies = optional;
writeJson(mainPath, main);

const names = Object.keys(optional);
console.log(`synced ${names.length} platform package(s) to ${version}:`);
for (const n of names) console.log(`  ${n}@${version}`);
