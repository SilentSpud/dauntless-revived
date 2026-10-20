// Only reconcile this bot's exact invite. An incomplete history is never proof of absence.
export async function findInviteMessage(user, code) {
  const channel = await user.createDM();
  let before;
  for (let page = 0; page < 20; page++) {
    const messages = await channel.messages.fetch({limit:100, ...(before ? {before} : {})});
    const match = [...messages.values()].find(m => m.author.id === user.client.user.id &&
      (m.content.includes(`code=${code}`) || m.content.includes(`\n${code}\n`)));
    if (match) return {id:match.id, createdAt:match.createdAt.toISOString()};
    if (messages.size < 100) return null;
    before = messages.last().id;
  }
  return undefined;
}
