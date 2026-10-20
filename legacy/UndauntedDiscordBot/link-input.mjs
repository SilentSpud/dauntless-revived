// Accept the same legacy key alphabet as the launcher, plus private backup/Discord formatting.
export function linkInput(input) {
  if (typeof input !== 'string' || input.length > 8192) return {status:'invalid_key_format'};
  let text=input.trim();
  if (/dauntless-revived:\/\/join/i.test(text)) return {status:'invite_not_key'};
  if (text.startsWith('```') && text.endsWith('```')) text=text.slice(3,-3).trim();
  else if ((text.startsWith('`') && text.endsWith('`')) || (text.startsWith('"') && text.endsWith('"'))) text=text.slice(1,-1).trim();
  const labelled=[...text.matchAll(/^\s*Key:\s*([A-Za-z0-9_-]{8,128})\s*$/gm)];
  if (labelled.length > 1) return {status:'invalid_key_format'};
  if (labelled.length === 1) text=labelled[0][1];
  if (!/^[A-Za-z0-9_-]{8,128}$/.test(text)) return {status:'invalid_key_format'};
  return {key:text};
}

export const keyInstructions='In the launcher, open **Settings → Save a backup of your key…**. Open the saved text file and copy only the value after **Key:** into `/key link key:`. This uses your existing account; your key and progress stay unchanged. Do not post the key in chat.';
