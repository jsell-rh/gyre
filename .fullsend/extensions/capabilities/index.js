import { execFileSync } from 'node:child_process';

// Runs before a model request, using measured sandbox behavior rather than
// asking the implementer to infer infrastructure capabilities from failures.
export default function (pi) {
  let measured;
  pi.on('before_agent_start', (event) => {
    measured ??= execFileSync('python3', ['/tmp/stage/pipeline-remote.py', 'capabilities'], {
      encoding: 'utf8', timeout: 20000,
    }).trim();
    return { systemPrompt: event.systemPrompt + '\nMeasured Gyre runtime capabilities: ' + measured +
      '\nRead /tmp/stage/capabilities.json before expensive server/browser probes. ' +
      'An unsupported listener is infrastructure evidence for host verification, not a code defect. ' +
      'For build commands use /tmp/stage/bin/cargo, /tmp/stage/bin/npm, and /tmp/stage/bin/npx; these serialize builds within this sandbox.' };
  });
}
