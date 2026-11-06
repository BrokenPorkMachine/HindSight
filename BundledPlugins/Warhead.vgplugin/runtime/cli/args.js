/** Validate options before reading config or making any model requests. */
export function parseCliArgs(raw, diagnostics = false) {
    const result = { args: [] };
    for (let i = 0; i < raw.length; i++) {
        const option = raw[i];
        if (diagnostics && (option === 'rea' || option === '--rea')) {
            result.args.push(...raw.slice(i));
            break;
        }
        if (option === '--vllm') {
            result.vllm = true;
            continue;
        }
        const key = option === '--host' || option === '--ollama-host' ? 'host'
            : option === '--model' || option === '--ollama-model' ? 'model' : null;
        if (key) {
            const value = raw[++i];
            if (!value?.trim() || value.startsWith('-'))
                throw new Error(`${option} requires a value. Run warhead-cli --help for usage.`);
            result[key] = value.trim();
        }
        else if (diagnostics && (option === '--scan' || option === '--tools'))
            result.args.push(option);
        else
            throw new Error(`Unknown argument: ${option}. Run warhead-cli --help for usage.`);
    }
    return result;
}
