#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');

const executableName = process.platform === 'win32' ? 'exoroute.exe' : 'exoroute';
const executablePath = path.join(__dirname, '..', 'vendor', executableName);

try {
  const metadata = fs.lstatSync(executablePath);
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new Error('the installed binary is not a regular file');
  }
} catch (error) {
  console.error(`ExoRoute binary is not installed: ${error.message}`);
  console.error('Run npm install -g exoroute again with network access.');
  process.exit(1);
}

const child = spawn(executablePath, process.argv.slice(2), {
  cwd: process.cwd(),
  env: process.env,
  stdio: 'inherit',
  windowsHide: false
});

child.once('error', (error) => {
  console.error(`Could not start ExoRoute: ${error.message}`);
  process.exitCode = 1;
});

child.once('exit', (code, signal) => {
  if (signal) {
    process.exitCode = 1;
  } else {
    process.exitCode = code === null ? 1 : code;
  }
});
