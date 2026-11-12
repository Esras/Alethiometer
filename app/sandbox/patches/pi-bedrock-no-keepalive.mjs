// PATCH pi 1.4.0 until earendil-works/pi#11527: no keep-alive on Bedrock's HTTP/1 handler.
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = process.argv[2];
const files = [
  ['dist/bundle/chunks/bedrock-converse-stream.js', /(AWS_BEDROCK_FORCE_HTTP1",options\.env\)==="1"&&\(config\.requestHandler=new [\w$]+\.NodeHttpHandler)\)/],
  ['node_modules/@earendil-works/pi-ai/dist/api/bedrock-converse-stream.js', /(config\.requestHandler = new NodeHttpHandler)\(\);/],
];
for (const [file, find] of files) {
  const path = join(root, file);
  const src = readFileSync(path, 'utf8');
  if (!find.test(src)) throw new Error(`pi-bedrock-no-keepalive: ${file} has changed; check whether #11527 is fixed`);
  writeFileSync(
    path,
    src.replace(find, (_, head) =>
      file.includes('bundle') ? `${head}({httpsAgent:{keepAlive:false}}))` : `${head}({ httpsAgent: { keepAlive: false } });`,
    ),
  );
  console.log(`pi-bedrock-no-keepalive: patched ${file}`);
}
