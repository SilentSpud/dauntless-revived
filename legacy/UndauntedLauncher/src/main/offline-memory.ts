// Packaged Offline uses the same admission estimate here and in Deploy's env.
// This is a logical gate; Deploy still checks changing memory before each server.
export const OFFLINE_MEMORY_POLICY = Object.freeze({ minFreeMiB: 3072, serverStartupMiB: 1536 });
export const OFFLINE_REQUIRED_AVAILABLE_MIB = OFFLINE_MEMORY_POLICY.minFreeMiB + OFFLINE_MEMORY_POLICY.serverStartupMiB;
export const OFFLINE_REQUIRED_AVAILABLE_BYTES = OFFLINE_REQUIRED_AVAILABLE_MIB * 1024 * 1024;

export function hasOfflineMemory(availableBytes: number): boolean {
    return Number.isFinite(availableBytes) && availableBytes >= OFFLINE_REQUIRED_AVAILABLE_BYTES;
}
