/**
 * HindSight capability-negotiated adapter (P3).
 * Only uses advertised capabilities; no assumed remote debugging/upload/VM features.
 */
export class HindsightAdapter {
    config;
    constructor(config = {}) {
        this.config = {
            capabilities: config.capabilities ?? ['project_read'],
            allowRemoteDebug: false,
            allowUpload: false,
            allowVmExecution: false,
            ...config,
        };
        if (this.config.allowRemoteDebug || this.config.allowUpload || this.config.allowVmExecution) {
            throw new Error('HindSight elevated capabilities require separate explicit grants not enabled by default');
        }
    }
    getCapabilities() {
        return [...(this.config.capabilities ?? [])];
    }
    async analyze(_request) {
        if (!this.config.capabilities?.includes('async_analysis')) {
            return { status: 'denied', reason: 'async_analysis not advertised by connected HindSight instance' };
        }
        return { status: 'unavailable', reason: 'HindSight adapter not connected in this environment' };
    }
}
export const createHindsightAdapter = (config) => new HindsightAdapter(config);
