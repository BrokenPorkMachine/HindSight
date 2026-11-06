const CORE = ['read_file', 'glob', 'grep', 'todo', 'edit', 'write', 'multi_edit', 'bash'];
const EXTRAS = [
    { match: /\b(?:search (?:the )?(?:web|internet)|online|webpage|website|url|https?:\/\/|documentation|api docs|reference|how to)\b/i,
        names: ['web_search', 'fetch_web_page'] },
    { match: /\b(?:bulk edit|multiple edits|multi.edit|edit multiple files|rename across files|refactor|mass change)\b/i,
        names: ['multi_edit'] },
    { match: /\b(?:reverse\.?engineer|binary analy|disassembl|decompil|debug|trace|exploitat|vuln|poc|shellcode|rop|heap|stack|overflow|format string|hindsight|firmware|patch|crack|unpack)\b/i,
        names: ['hindsight_context'] },
    { match: /\b(?:compress|archive|zip|shrink|pack|unpack)\b/i,
        names: ['compress_file'] },
    { match: /\b(?:fast\.?search|codebase search|semantic search|find symbol|find definition|find usage|call graph|xref|cross reference)\b/i,
        names: ['fast_search'] },
];
export function nativeToolsForTurn(tools, request) {
    const selected = new Set(CORE);
    for (const extra of EXTRAS)
        if (extra.match.test(request)) {
            for (const name of extra.names)
                selected.add(name);
        }
    // Prefer the declared registry order: the model cannot invoke tools omitted
    // from its schema, but all supplied calls still require authorization.
    return tools.filter(tool => selected.has(tool.def.name));
}
