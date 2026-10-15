import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';

if (process.platform === 'win32') {
  const release = process.argv.includes('--release') && process.env.TAURI_ENV_DEBUG !== 'true';
  const target = process.env.TAURI_ENV_TARGET_TRIPLE;
  const args = ['build', '--locked', '-p', 'mangodisk-privileged'];
  if (release) args.push('--release');
  if (target) args.push('--target', target);
  const result = spawnSync('cargo', args, { stdio: 'inherit' });
  if (result.status !== 0) process.exit(result.status ?? 1);
  const directory = resolve('src-tauri/resources/privileged');
  mkdirSync(directory, { recursive: true });
  const helper = resolve(directory, 'MangoDiskPrivileged.exe');
  copyFileSync(
    resolve(
      process.env.CARGO_TARGET_DIR ?? 'target',
      ...(target ? [target] : []),
      release ? 'release' : 'debug',
      'MangoDiskPrivileged.exe'
    ),
    helper
  );
  if (release) {
    // Release tooling supplies a trusted argv array; no shell interpolation.
    const signCommand = process.env.MANGODISK_PRIVILEGED_SIGN_COMMAND;
    if (signCommand) {
      const command = JSON.parse(signCommand);
      if (!Array.isArray(command) || command.length < 2 || !command.every(arg => typeof arg === 'string')) {
        throw new Error('MANGODISK_PRIVILEGED_SIGN_COMMAND must be a JSON argv array containing {file}');
      }
      if (!command.slice(1).includes('{file}')) {
        throw new Error('MANGODISK_PRIVILEGED_SIGN_COMMAND requires a {file} argument');
      }
      const signed = spawnSync(
        command[0],
        command.slice(1).map(arg => (arg === '{file}' ? helper : arg)),
        {
          stdio: 'inherit',
        }
      );
      if (signed.status !== 0) process.exit(signed.status ?? 1);
    }
    // Windows PowerShell must load its own inbox security module even when the
    // build inherits PowerShell 7's incompatible module search path.
    const verificationEnvironment = { ...process.env, MANGODISK_PRIVILEGED_VERIFY_PATH: helper };
    delete verificationEnvironment.PSModulePath;
    const verified = spawnSync(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        '$signature = Get-AuthenticodeSignature -LiteralPath $env:MANGODISK_PRIVILEGED_VERIFY_PATH; if ($signature.Status -ne "Valid") { Write-Error "The release privileged helper requires valid Authenticode signing before packaging"; exit 1 }',
      ],
      { stdio: 'inherit', env: verificationEnvironment }
    );
    if (verified.status !== 0) process.exit(verified.status ?? 1);
  }
}
