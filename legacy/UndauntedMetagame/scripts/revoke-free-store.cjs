// Run only during maintenance with a database backup and the metagame env loaded.
const {createHash}=require('node:crypto');
const {GetDb}=require('../dist/db');
const {RevokeStorePurchasesBefore}=require('../dist/controllers/storerevocation');
const cutoff=process.argv[2];
const apply=process.argv.includes('--apply');
const db=GetDb().$client;
const progressionHash=()=>{
  const hash=createHash('sha256');
  for(const table of ['characters','characterhistory','escalationprogression','escalationtalents','escalationunlocks'])
    hash.update(JSON.stringify(db.prepare(`SELECT * FROM ${table} ORDER BY rowid`).all()));
  return hash.digest('hex');
};
try {
  const before=progressionHash();
  const preview=RevokeStorePurchasesBefore(cutoff);
  console.log(JSON.stringify({mode:'preview',cutoff,...preview}));
  if(apply) {
    // Outer transaction rolls back if unrelated progress unexpectedly changes.
    db.transaction(()=>{
      const result=RevokeStorePurchasesBefore(cutoff,true);
      if(before!==progressionHash())throw new Error('Unrelated character or escalation data changed');
      console.log(JSON.stringify({mode:'applied',cutoff,...result,progressionUnchanged:true}));
    })();
    const repeat=RevokeStorePurchasesBefore(cutoff);
    if(repeat.purchases!==0)throw new Error('Revocation audit did not cover all purchases');
    console.log(JSON.stringify({mode:'verified',remaining:repeat.purchases,integrity:db.pragma('quick_check',{simple:true})}));
  }
} finally {db.close();}
