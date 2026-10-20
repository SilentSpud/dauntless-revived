// Import this first in every test file: quiet, plain JSON logs, and no pino-pretty worker.
process.env.NODE_ENV = "production";
process.env.LOG_LEVEL = process.env.TEST_LOG_LEVEL ?? "silent";
// Fake-process suites must not depend on the CI host's free RAM. Capacity tests explicitly enable it.
process.env.GAMESERVER_MEMORY_GUARD = '0';
process.env.GAMESERVER_STARTUP_GRACE_MS = '0';

export {};
