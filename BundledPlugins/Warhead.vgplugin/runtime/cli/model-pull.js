/** Direct model acquisition before starting the interactive agent. */
import { pullHuggingFace, pullOllamaNative } from '../model/pull-native.js';
export async function runModelPull(args) {
    const [source, name, ...flags] = args;
    if (!name || (source !== 'hf' && source !== 'ollama')) {
        throw new Error('usage: warhead-cli model pull hf <owner/repo> [--file model.gguf | --mlx] OR warhead-cli model pull ollama <tag>');
    }
    const index = flags.indexOf('--file');
    if (index >= 0 && !flags[index + 1])
        throw new Error('--file requires a repository-relative GGUF filename');
    const supported = flags.every((v, i) => v === '--mlx' || v === '--file' || (i > 0 && flags[i - 1] === '--file'));
    if (!supported || (source === 'ollama' && flags.length))
        throw new Error('unrecognized model pull option');
    const report = (message) => process.stderr.write(message + '\n');
    const result = source === 'hf'
        ? await pullHuggingFace(name, { ...(flags.includes('--mlx') ? { engine: 'mlx' } : {}), ...(index >= 0 ? { file: flags[index + 1] } : {}), report })
        : await pullOllamaNative(name, { report });
    process.stdout.write(`Downloaded ${result.files} file(s), ${result.downloadedBytes} bytes\nNative model: ${result.ref}\nRun: warhead-cli --model "${result.ref}"\n`);
}
if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
    runModelPull(process.argv.slice(2)).catch(error => {
        process.stderr.write(`model pull: ${error instanceof Error ? error.message : String(error)}\n`);
        process.exitCode = 1;
    });
}
