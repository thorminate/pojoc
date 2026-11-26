#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");
const { constants } = require("node:os");
const { name, optionalDependencies } = require("./package.json");

const platform = `${process.platform}-${process.arch}`;
const binaryPackage = `${name}-${platform}`;
if (!Object.hasOwn(optionalDependencies, binaryPackage)) {
  console.error(`${name} does not provide a binary for ${platform}.`);
  process.exit(1);
}

let executable;
try {
  const filename = process.platform === "win32" ? "pojoc.exe" : "pojoc";
  executable = require.resolve(`${binaryPackage}/bin/${filename}`);
} catch (error) {
  if (error.code !== "MODULE_NOT_FOUND") throw error;
  console.error(`Cannot find ${binaryPackage}. Install ${name} with optional dependencies enabled.`);
  process.exit(1);
}

const result = spawnSync(executable, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`Could not launch Pojoc: ${result.error.message}`);
  process.exitCode = 1;
} else {
  const signal = result.signal && constants.signals[result.signal];
  process.exitCode = result.status ?? (signal ? 128 + signal : 1);
}
