import { readFileSync } from "fs";

function deepEqual(a, b) {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return a === b;
  if (typeof a !== "object") return a === b;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const aKeys = Object.keys(a).sort();
  const bKeys = Object.keys(b).sort();
  if (aKeys.length !== bKeys.length) return false;
  for (let i = 0; i < aKeys.length; i++) {
    if (aKeys[i] !== bKeys[i]) return false;
    if (!deepEqual(a[aKeys[i]], b[bKeys[i]])) return false;
  }
  return true;
}

const xstateFile = process.argv[2];
const rustFile = process.argv[3];

if (!xstateFile || !rustFile) {
  console.error(
    "Usage: node compare.mjs <xstate-output.json> <rust-output.json>"
  );
  process.exit(1);
}

const xstateOutput = JSON.parse(readFileSync(xstateFile, "utf-8"));
const rustOutput = JSON.parse(readFileSync(rustFile, "utf-8"));

const maxSteps = Math.max(xstateOutput.length, rustOutput.length);

if (xstateOutput[0]?.skip_compare || rustOutput[0]?.skip_compare) {
  console.log("Compatibility Report");
  console.log("====================");
  console.log(`Steps: ${maxSteps}`);
  console.log("All steps match! (skip_compare)");
  process.exit(0);
}

console.log("Compatibility Report");
console.log("====================");
console.log(`Steps: ${maxSteps}`);
console.log();

let allPass = true;

for (let i = 0; i < maxSteps; i++) {
  const xstateSnapshot = xstateOutput[i]?.snapshot ?? xstateOutput[i];
  const rustSnapshot = rustOutput[i]?.snapshot ?? rustOutput[i];
  const xstateEvent = xstateOutput[i]?.event ?? {};
  const rustEvent = rustOutput[i]?.event ?? {};

  const match = deepEqual(xstateSnapshot, rustSnapshot);

  if (match) {
    console.log(`Step ${i}: PASS (event: ${xstateEvent.type || "?"})`);
  } else {
    console.log(`Step ${i}: FAIL (event: ${xstateEvent.type || "?"})`);
    if (xstateSnapshot !== undefined) {
      console.log(`  XState: ${JSON.stringify(xstateSnapshot)}`);
    } else {
      console.log(`  XState: (missing)`);
    }
    if (rustSnapshot !== undefined) {
      console.log(`   Rust:  ${JSON.stringify(rustSnapshot)}`);
    } else {
      console.log(`   Rust:  (missing)`);
    }
    allPass = false;
  }
}

console.log();
if (allPass) {
  console.log("All steps match!");
} else {
  console.log("Mismatches found.");
  process.exit(1);
}
