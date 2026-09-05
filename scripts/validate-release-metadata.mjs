import { access, readFile } from "node:fs/promises";
import path from "node:path";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const tauriRoot = path.join(repositoryRoot, "src-tauri");

const [
  packageJson,
  packageLock,
  tauriConfig,
  cargoManifest,
  cargoLock,
  ciWorkflow,
  packageWorkflow,
  releaseWorkflow,
] = await Promise.all([
  readJson(path.join(repositoryRoot, "package.json")),
  readJson(path.join(repositoryRoot, "package-lock.json")),
  readJson(path.join(tauriRoot, "tauri.conf.json")),
  readFile(path.join(tauriRoot, "Cargo.toml"), "utf8"),
  readFile(path.join(tauriRoot, "Cargo.lock"), "utf8"),
  readFile(path.join(repositoryRoot, ".github/workflows/ci.yml"), "utf8"),
  readFile(
    path.join(repositoryRoot, ".github/workflows/package-unsigned.yml"),
    "utf8",
  ),
  readFile(path.join(repositoryRoot, ".github/workflows/release.yml"), "utf8"),
]);

const cargoPackage = cargoManifest
  .split(/^\[package\]\s*$/m)[1]
  ?.split(/^\[/m)[0];
assert(cargoPackage, "Cargo.toml must contain a [package] table.");
const cargoLockPackage = cargoLock
  .split(/^\[\[package\]\]\s*$/m)
  .find((table) => quotedTomlValue(table, "name") === "twominal");
assert(cargoLockPackage, "Cargo.lock must contain the twominal package.");

const cargoVersion = quotedTomlValue(cargoPackage, "version");
const cargoLockVersion = quotedTomlValue(cargoLockPackage, "version");
const cargoName = quotedTomlValue(cargoPackage, "name");
const cargoRustVersion = quotedTomlValue(cargoPackage, "rust-version");

assert(packageJson.name === "twominal", "package.json name must be twominal.");
assert(packageJson.private === true, "The npm package must remain private.");
assert(
  packageJson.packageManager === "npm@10.9.8",
  "package.json must pin the supported npm version.",
);
assert(
  packageJson.engines?.node === ">=22.13 <23" &&
    packageJson.engines?.npm === ">=10.9 <11",
  "package.json must declare the supported Node.js 22 and npm 10 ranges.",
);
assert(cargoName === "twominal", "Cargo package name must be twominal.");
assert(
  /^publish\s*=\s*false\s*$/m.test(cargoPackage),
  "The desktop crate must not be publishable to crates.io.",
);
assert(
  cargoRustVersion === "1.88",
  "Cargo rust-version must remain at the declared MSRV.",
);

const ciRustVersion = workflowEnvironment(ciWorkflow, "TWOMINAL_RUST_VERSION");
const packageRustVersion = workflowEnvironment(
  packageWorkflow,
  "TWOMINAL_RUST_VERSION",
);
const ciNodeVersion = workflowEnvironment(ciWorkflow, "TWOMINAL_NODE_VERSION");
const packageNodeVersion = workflowEnvironment(
  packageWorkflow,
  "TWOMINAL_NODE_VERSION",
);
const releaseNodeVersion = workflowEnvironment(
  releaseWorkflow,
  "TWOMINAL_NODE_VERSION",
);
assert(
  ciRustVersion === `${cargoRustVersion}.0` &&
    packageRustVersion === ciRustVersion,
  "Cargo MSRV and both workflow Rust toolchains must remain aligned.",
);
assert(
  ciNodeVersion === "22.23.2" &&
    packageNodeVersion === ciNodeVersion &&
    releaseNodeVersion === ciNodeVersion,
  "Validation, packaging, and release workflows must use the same pinned Node.js 22 release.",
);
assert(
  /^\s*workflow_call:\s*$/m.test(packageWorkflow),
  "The unsigned packaging workflow must remain reusable by the release workflow.",
);

const versionSources = [
  ["package.json", packageJson.version],
  ["package-lock.json", packageLock.version],
  ["package-lock.json root package", packageLock.packages?.[""]?.version],
  ["Cargo.toml", cargoVersion],
  ["Cargo.lock", cargoLockVersion],
  ["tauri.conf.json", tauriConfig.version],
];
for (const [source, version] of versionSources) {
  assert(
    typeof version === "string" && /^\d+\.\d+\.\d+$/.test(version),
    `${source} must contain a plain semantic version.`,
  );
}
assert(
  versionSources.every(([, version]) => version === packageJson.version),
  "Release manifest and lock-file versions must match.",
);

assert(
  tauriConfig.productName === "Twominal",
  "Tauri productName must remain Twominal.",
);
assert(
  tauriConfig.identifier === "com.twominal.desktop",
  "Changing the application identifier requires an explicit migration.",
);
assert(tauriConfig.bundle?.active === true, "Tauri bundling must be enabled.");
assert(
  tauriConfig.bundle?.createUpdaterArtifacts === false,
  "Unsigned CI packages must not create updater artifacts.",
);
assert(
  tauriConfig.bundle?.category === "DeveloperTool",
  "Tauri bundle category must describe a developer tool.",
);
assert(
  tauriConfig.bundle?.macOS?.minimumSystemVersion === "10.13",
  "The declared minimum macOS version must remain explicit.",
);

await Promise.all([
  ...requiredFiles(tauriConfig.bundle?.resources, "bundle.resources"),
  ...requiredFiles(tauriConfig.bundle?.icon, "bundle.icon"),
]);

globalThis.process.stdout.write(
  `Release metadata valid: Twominal ${packageJson.version}, Rust ${cargoRustVersion}.\n`,
);

function requiredFiles(values, fieldName) {
  assert(
    Array.isArray(values) && values.length > 0,
    `${fieldName} must be a non-empty array.`,
  );
  return values.map(async (relativePath) => {
    assert(
      typeof relativePath === "string" && relativePath.length > 0,
      `${fieldName} entries must be non-empty strings.`,
    );
    const resolved = path.resolve(tauriRoot, relativePath);
    const relative = path.relative(tauriRoot, resolved);
    assert(
      relative !== "" &&
        relative !== ".." &&
        !relative.startsWith(`..${path.sep}`) &&
        !path.isAbsolute(relative),
      `${fieldName} entries must stay inside src-tauri.`,
    );
    await access(resolved).catch(() => {
      throw new Error(`${fieldName} references a missing file: ${relativePath}`);
    });
  });
}

function quotedTomlValue(table, key) {
  const escapedKey = key.replaceAll("-", "\\-");
  return table.match(new RegExp(`^${escapedKey}\\s*=\\s*"([^"]+)"\\s*$`, "m"))?.[1];
}

function workflowEnvironment(workflow, key) {
  const escapedKey = key.replaceAll(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return workflow.match(new RegExp(`^\\s*${escapedKey}:\\s*"([^"]+)"\\s*$`, "m"))?.[1];
}

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, "utf8"));
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}
