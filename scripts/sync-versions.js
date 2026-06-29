const fs = require('fs');
const path = require('path');

const version = process.argv[2];
if (!version) {
  console.error("Usage: node sync-versions.js <version>");
  process.exit(1);
}

const rootDir = path.join(__dirname, '..');

// 1. Root package.json
const rootPkgPath = path.join(rootDir, 'npm', 'package.json');
const rootPkg = JSON.parse(fs.readFileSync(rootPkgPath, 'utf8'));
rootPkg.version = version;

// Update optionalDependencies versions
if (rootPkg.optionalDependencies) {
  for (const dep of Object.keys(rootPkg.optionalDependencies)) {
    if (dep.startsWith('@codebase-synapse/index-')) {
      rootPkg.optionalDependencies[dep] = version;
    }
  }
}
fs.writeFileSync(rootPkgPath, JSON.stringify(rootPkg, null, 2) + '\n');
console.log(`Updated npm/package.json to version ${version}`);

// 2. Platform packages
const platformPackagesDir = path.join(rootDir, 'npm', 'platform-packages');
const platforms = ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-arm64', 'win32-x64'];

for (const platform of platforms) {
  const pkgPath = path.join(platformPackagesDir, platform, 'package.json');
  if (fs.existsSync(pkgPath)) {
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    pkg.version = version;
    delete pkg.private; // Ensure it is not private so npm publish succeeds!
    fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');
    console.log(`Updated npm/platform-packages/${platform}/package.json to version ${version}`);
  } else {
    console.warn(`Warning: Platform package config not found: ${pkgPath}`);
  }
}
