/**
 * Shared MCP protocol helpers.
 */
export function asObject(value) {
    return typeof value === 'object' && value !== null ? value : {};
}
export function responseText(result) {
    const blocks = Array.isArray(result['content']) ? result['content'] : [];
    const parts = blocks.flatMap((block) => {
        const item = asObject(block);
        if (item['type'] === 'text' && typeof item['text'] === 'string')
            return [item['text']];
        if (item['type'] === 'image')
            return ['[image result omitted]'];
        return [];
    });
    if (parts.length)
        return parts.join('\n');
    if (result['structuredContent'] !== undefined)
        return JSON.stringify(result['structuredContent'], null, 2);
    return '(MCP tool returned no text)';
}
export class McpError extends Error {
    constructor(message) {
        super(message);
        this.name = 'McpError';
    }
}
