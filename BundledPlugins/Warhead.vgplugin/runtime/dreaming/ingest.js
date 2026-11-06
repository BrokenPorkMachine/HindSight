/**
 * Observation-only ingestion (D1 stub). Disabled by default.
 */
export class DreamIngester {
    enabled;
    constructor(enabled = false) { this.enabled = enabled; }
    isEnabled() { return this.enabled; }
    async ingest(_events) {
        if (!this.enabled)
            return [];
        throw new Error('Dreaming ingestion not implemented');
    }
    deduplicate(_findings) {
        // Deduplication by fingerprint required per spec
        return _findings;
    }
}
