import { and, eq, isNotNull, isNull, lte } from 'drizzle-orm';
import { GetDb } from '../db';
import { entitlements, inventory, inventorylog, storepurchases } from '../db/schema';
import { ApplyInventoryTransactionInTx } from './inventory';

// Maintenance only: stop game processes and the metagame first so cached inventories
// cannot restore revoked grants. Preview first, back up, then apply the same cutoff.
export function RevokeStorePurchasesBefore(cutoff: string, apply = false) {
    if (!Number.isFinite(Date.parse(cutoff))) throw new Error('Invalid cutoff');
    cutoff = new Date(cutoff).toISOString();
    return GetDb().transaction(tx => {
        const report = { purchases: 0, characters: 0, stackedUnits: 0, instances: 0, entitlements: 0, alreadyProcessed: 0 };
        const affected = new Set<string>();
        const purchases = tx.select().from(storepurchases).where(and(isNotNull(storepurchases.redeemedDate), lte(storepurchases.redeemedDate, cutoff))).all();
        for (const purchase of purchases) {
            const transactionId = `store-revoke:${purchase.tokenHash}`;
            if (tx.select().from(inventorylog).where(and(eq(inventorylog.transactionId, transactionId), eq(inventorylog.operation, 'store_revoke'))).get()) {
                report.alreadyProcessed++;
                continue;
            }
            const entries = tx.select().from(inventorylog).where(and(eq(inventorylog.characterId, purchase.characterId), eq(inventorylog.transactionId, `store:${purchase.tokenHash}`), eq(inventorylog.caller, 'store'))).all();
            if (entries.some(e => e.operation === 'remove' && (e.quantityChange ?? 0) < 0)) throw new Error('Paid purchase encountered; this tool only revokes free grants');
            const grants = entries.filter(e => e.operation === 'add');
            const row = tx.select().from(inventory).where(eq(inventory.characterId, purchase.characterId)).get();
            const stacks: any[] = JSON.parse(row?.stackedItems ?? '[]');
            const instances: any[] = JSON.parse(row?.instancedItems ?? '[]');
            const stacked = grants.filter(g => !g.instanceId && g.catalogId && (g.quantityChange ?? 0) > 0).map(g => ({catalogId: g.catalogId!, quantity: Math.min(g.quantityChange!, Number(stacks.find(s => s.catalogId === g.catalogId)?.quantity ?? 0))})).filter(g => g.quantity > 0);
            const instanced = grants.filter(g => g.instanceId).flatMap(g => {
                const item = instances.find(i => i.instanceId === g.instanceId && i.catalogId === g.catalogId);
                return item ? [{...item, updateVersion: Number(item.updateVersion ?? 0) + 1}] : [];
            });
            const owned = tx.select().from(entitlements).where(and(eq(entitlements.accountId, purchase.accountId), eq(entitlements.source, `store:${purchase.skuId}`), isNull(entitlements.revokedDate), lte(entitlements.grantedDate, cutoff))).all();
            report.purchases++;
            report.stackedUnits += stacked.reduce((n, s) => n + s.quantity, 0);
            report.instances += instanced.length;
            report.entitlements += owned.length;
            affected.add(purchase.characterId);
            if (!apply) continue;
            if (stacked.length || instanced.length) ApplyInventoryTransactionInTx(tx, {
                UserId: purchase.accountId, CharacterId: purchase.characterId, TransactionId: transactionId,
                StackedItemsToRemove: stacked, InstancedItemsToRemove: instanced
            }, {Caller: 'admin', Source: 'free-store-revocation'});
            for (const entitlement of owned) tx.update(entitlements).set({revokedDate: new Date().toISOString()}).where(and(eq(entitlements.accountId, entitlement.accountId), eq(entitlements.name, entitlement.name))).run();
            // Append-only marker survives the transient transaction-receipt retention period.
            tx.insert(inventorylog).values({time: new Date().toISOString(), userId: purchase.accountId, characterId: purchase.characterId, transactionId, caller: 'admin', source: 'free-store-revocation', operation: 'store_revoke'}).run();
        }
        if (apply) tx.update(storepurchases).set({expiresDate: cutoff}).where(and(isNull(storepurchases.redeemedDate), lte(storepurchases.createdDate, cutoff))).run();
        report.characters = affected.size;
        return report;
    });
}
