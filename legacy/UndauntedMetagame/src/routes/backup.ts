import {Router, type RequestHandler} from 'express';
import path from 'node:path';
import {realpath, lstat} from 'node:fs/promises';
import {IsDirectLocalRequest} from '../middleware/RequestOrigin';
import {IsValidGameserverAPIKey} from '../controllers/apikeys';
import {GetDb} from '../db';
import {logger} from '../logger';
import {rateLimit} from 'express-rate-limit';

type Options = {
    root: () => string | undefined;
    validKey: (key: string) => Promise<boolean>;
    copy: (destination: string, progress: () => number) => Promise<unknown>;
};

export function CreateBackupRouter(options: Options) {
    const router = Router();
    let active = false;
    // Backups are expensive writes, not a polling endpoint. Share one host-wide
    // budget across valid keys; rejected callers cannot consume it.
    const BackupRateLimit = rateLimit({
        windowMs: 60 * 60 * 1000, limit: 6, keyGenerator: () => 'local-backup',
        standardHeaders: 'draft-8', legacyHeaders: false,
        message: {error: 'rate_limited', message: 'Too many backup requests; try again later.'}
    });
    const LocalBackupAuth: RequestHandler = async function LocalBackupAuth(req, res, next) {
        if (!IsDirectLocalRequest(req)) { res.sendStatus(403); return; }
        const root = options.root();
        if (!root) { res.sendStatus(404); return; }
        const key = req.headers['x-undaunted-gameserver-apikey'];
        if (typeof key !== 'string' || !await options.validKey(key)) { res.sendStatus(403); return; }
        next();
    };
    router.post('/internal/backup', LocalBackupAuth, BackupRateLimit, async (req, res) => {
        const root = options.root()!;
        const name = req.body?.name;
        if (typeof name !== 'string' || !/^\d{4}-\d{2}-\d{2}_\d{6}$/.test(name)) { res.sendStatus(400); return; }
        if (active) { res.sendStatus(409); return; }
        active = true;
        try {
            const base = await realpath(root);
            const requested = path.join(base, name);
            if (await realpath(requested) !== requested) { res.sendStatus(400); return; }
            const destination = path.join(requested, 'undaunted.db');
            const exists = await lstat(destination).then(() => true, error => {
                if (error.code === 'ENOENT') return false;
                throw error;
            });
            if (exists) { res.sendStatus(409); return; }
            // Multi-gigabyte production databases need more than a minute on VPS disks.
            const deadline = Date.now() + 15 * 60 * 1000;
            // Use the writer's connection: external connections restart on each live save.
            await options.copy(destination, () => {
                if (Date.now() >= deadline) throw new Error('Live backup deadline exceeded');
                return 100;
            });
            res.json({copied: true});
        } catch (error) {
            logger.error({err: error}, 'Internal database backup failed');
            res.sendStatus(500);
        } finally { active = false; }
    });
    return router;
}

export const backupRouter = CreateBackupRouter({
    root: () => process.env.BACKUP_ROOT,
    validKey: IsValidGameserverAPIKey,
    copy: (destination, progress) => GetDb().$client.backup(destination, {progress})
});
