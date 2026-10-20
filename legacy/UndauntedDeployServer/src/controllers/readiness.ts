import { readFile, unlink } from 'node:fs/promises';
import { setTimeout } from 'node:timers/promises';
import { createSocket } from 'node:dgram';
import type { ChildProcess } from 'node:child_process';

function ChildStopped(child: ChildProcess) {
    return child.exitCode !== null && child.exitCode !== undefined || child.signalCode !== null && child.signalCode !== undefined;
}

export async function IsUdpPortBound(port: number) {
    return new Promise<boolean>((resolve, reject) => {
        const socket = createSocket({ type: 'udp4', reuseAddr: false });
        let settled = false;

        socket.once('error', (error: NodeJS.ErrnoException) => {
            if(settled) return;
            settled = true;
            try { socket.close(); } catch {}
            if (error.code === 'EADDRINUSE' || error.code === 'EACCES') resolve(true);
            else reject(error);
        });

        socket.bind({ port, address: '0.0.0.0', exclusive: true }, () => {
            if(settled) return;
            settled = true;
            socket.removeAllListeners('error');
            socket.close(() => resolve(false));
        });
    });
}

export async function WaitForServerReady(child: ChildProcess, port: number, file: string, timeoutMs = 90000) {
    const deadline = Date.now() + timeoutMs;
    let boundSamples = 0;
    let failure: Error | undefined;
    const onExit = () => { failure = new Error(`Game server on port ${port} exited before listening`); };
    const onError = (error: Error) => { failure = new Error(`Game server on port ${port} failed before listening: ${error.message}`); };
    child.once('exit', onExit);
    child.once('error', onError);

    try {
        while (Date.now() < deadline) {
            if(failure) throw failure;
            if (ChildStopped(child)) throw new Error(`Game server on port ${port} exited before listening`);

            try {
                const marker = (await readFile(file, 'utf8')).trim();
                const parts = marker.split(':');
                if (parts.length === 2 && /^\d+$/.test(parts[0]) && Number(parts[1]) === port) {
                    if(failure) throw failure;
                    if(ChildStopped(child)) throw new Error(`Game server on port ${port} exited before listening`);
                    return;
                }
            }
            catch (error) {
                if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
            }

            const Bound = await IsUdpPortBound(port);
            if(failure) throw failure;
            if(ChildStopped(child)) throw new Error(`Game server on port ${port} exited before listening`);

            if (Bound) {
                boundSamples++;
                if (boundSamples >= 2) return;
            }
            else {
                boundSamples = 0;
            }

            await setTimeout(Math.min(200, Math.max(1, deadline - Date.now())));
        }

        if(failure) throw failure;
        if(ChildStopped(child)) throw new Error(`Game server on port ${port} exited before listening`);
        throw new Error(`Game server on port ${port} did not start listening within ${timeoutMs}ms`);
    }
    finally {
        child.off('exit', onExit);
        child.off('error', onError);
        await unlink(file).catch(() => {});
    }
}
