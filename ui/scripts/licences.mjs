// Prints the licences of the npm packages whose code ends up in the Forge window: the production
// dependencies and Svelte (a dev dependency that Vite bundles into the page), each with the
// dependencies they need to run. Build and test tools are not shipped and are left out.
//
// Usage: node scripts/licences.mjs   (run in ui/, after `pnpm install`)
import { existsSync, readdirSync, readFileSync, realpathSync } from 'node:fs';
import { dirname, join } from 'node:path';

const BUNDLED_DEV_DEPENDENCIES = ['svelte'];

const readManifest = (folder) => JSON.parse(readFileSync(join(folder, 'package.json'), 'utf8'));

// The folder of `name` as seen from `from`: the nearest node_modules above it that has it.
function resolve(name, from) {
  for (let folder = from; ; folder = dirname(folder)) {
    const candidate = join(folder, 'node_modules', name);
    if (existsSync(join(candidate, 'package.json'))) return realpathSync(candidate);
    if (dirname(folder) === folder) throw new Error(`cannot find ${name} from ${from}`);
  }
}

const root = process.cwd();
const project = readManifest(root);
const wanted = [...Object.keys(project.dependencies ?? {}), ...BUNDLED_DEV_DEPENDENCIES];
for (const name of BUNDLED_DEV_DEPENDENCIES) {
  if (!project.devDependencies?.[name]) {
    throw new Error(`${name} is not a dev dependency any more; update BUNDLED_DEV_DEPENDENCIES`);
  }
}

// folder -> manifest, for the packages and everything they need to run
const packages = new Map();
function visit(name, from) {
  const folder = resolve(name, from);
  if (packages.has(folder)) return;
  const manifest = readManifest(folder);
  packages.set(folder, manifest);
  for (const dependency of Object.keys(manifest.dependencies ?? {})) visit(dependency, folder);
  for (const optional of Object.keys(manifest.optionalDependencies ?? {})) {
    try {
      visit(optional, folder);
    } catch {
      // not installed on this platform
    }
  }
}
for (const name of wanted) visit(name, root);

function licenceFile(folder) {
  const name = readdirSync(folder).find((file) => /^(licen[cs]e|copying|notice)(\.|$)/i.test(file));
  return name ? readFileSync(join(folder, name), 'utf8').trim() : null;
}

const entries = [...packages].map(([folder, manifest]) => ({ folder, manifest }));
entries.sort((a, b) =>
  `${a.manifest.name}@${a.manifest.version}`.localeCompare(
    `${b.manifest.name}@${b.manifest.version}`,
  ),
);

const out = ['Third-party licences (npm packages in the Forge window)', ''];
for (const { folder, manifest } of entries) {
  const key = `${manifest.name}@${manifest.version}`;
  const licence =
    typeof manifest.license === 'string' ? manifest.license : JSON.stringify(manifest.license);
  // A few packages ship no licence file; the licence they declare is given instead.
  const repository =
    typeof manifest.repository === 'string' ? manifest.repository : manifest.repository?.url;
  const text =
    licenceFile(folder) ??
    `No licence file is shipped with this package. Declared licence: ${licence}.${repository ? ` Source: ${repository}` : ''}`;
  out.push('='.repeat(80), `${key} (${licence})`, '', text, '');
}
process.stdout.write(out.join('\n') + '\n');
