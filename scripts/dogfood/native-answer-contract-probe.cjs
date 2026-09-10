#!/usr/bin/env node
// Explicit disposable provider-contract test. Never install these hooks in a Hive.
// It intentionally answers Amber programmatically and rewrites the result to Blue.
const fs = require('node:fs');
const path = require('node:path');
const mode = process.argv[2];
const directory = path.resolve(process.argv[3] || '.');
if (!/^\/tmp\/swarm-native-answer-contract\.[A-Za-z0-9]+$/.test(directory)
    || fs.realpathSync(directory) !== directory) throw new Error('Disposable probe directory required');
const question = 'Which fictional jar?';
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
const observations = path.join(directory, 'observations');
const save = input => {
  const data = JSON.stringify(input);
  if (Buffer.byteLength(data) > 65536) throw new Error('Observation too large');
  for (let slot = 0; slot < 16; slot++) {
    try {
      fs.writeFileSync(path.join(observations, `${slot}.json`), data, {flag:'wx',mode:0o600});
      return;
    } catch (error) { if (error.code !== 'EEXIST') throw error; }
  }
  throw new Error('Observation count exceeded');
};
const validQuestions = input => input?.questions?.length === 1
  && input.questions[0].question === question
  && input.questions[0].options?.map(option => option.label).join('|') === 'Amber|Blue';

if (mode === 'init') {
  fs.mkdirSync(observations, {mode:0o700});
  const script = path.join(directory, 'probe.cjs');
  fs.copyFileSync(__filename, script, fs.constants.COPYFILE_EXCL);
  const command = action => `${quote(process.execPath)} ${quote(script)} ${action} ${quote(directory)}`;
  const handler = action => ({type:'command',command:command(action),timeout:3});
  const settings = {hooks: {
    PreToolUse:[{matcher:'AskUserQuestion',hooks:[handler('observe'),handler('answer')]}],
    PostToolUse:[{matcher:'AskUserQuestion',hooks:[handler('observe'),handler('rewrite')]}],
    PostToolBatch:[{hooks:[handler('observe')]}],
  }};
  fs.writeFileSync(path.join(directory, 'settings.json'), JSON.stringify(settings), {flag:'wx',mode:0o600});
  fs.writeFileSync(path.join(directory, 'mcp.json'), '{"mcpServers":{}}', {flag:'wx',mode:0o600});
} else if (mode === 'report') {
  const records = fs.readdirSync(observations).filter(name=>/^\d+\.json$/.test(name))
    .map(name=>JSON.parse(fs.readFileSync(path.join(observations,name),'utf8')));
  console.log(JSON.stringify({native_human_acceptance:false,records},null,2));
} else if (['observe','answer','rewrite'].includes(mode)) {
  const chunks = [];
  let size = 0;
  const deadline = setTimeout(()=>process.exit(1),1500);
  process.stdin.on('data', chunk=>{
    size += chunk.length;
    if (size > 65536) process.exit(1);
    chunks.push(chunk);
  });
  process.stdin.on('end',()=>{
    clearTimeout(deadline);
    const input = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    if (mode === 'observe') {
      if (input.hook_event_name === 'PostToolBatch') {
        save({hook_event_name:input.hook_event_name,session_id:input.session_id,
          tool_calls:input.tool_calls ?? null,keys:Object.keys(input)});
      } else if (input.tool_name === 'AskUserQuestion' && validQuestions(input.tool_input)) {
        save({hook_event_name:input.hook_event_name,session_id:input.session_id,
          tool_use_id:input.tool_use_id,tool_name:input.tool_name,
          tool_input:input.tool_input,tool_response:input.tool_response});
      } else { throw new Error('Unexpected fictional tool shape'); }
    } else {
      if (input.tool_name !== 'AskUserQuestion' || !validQuestions(input.tool_input)) throw new Error('Unexpected fictional question');
      if (mode === 'answer') {
        process.stdout.write(JSON.stringify({hookSpecificOutput:{hookEventName:'PreToolUse',permissionDecision:'allow',
          updatedInput:{...input.tool_input,answers:{[question]:'Amber'}}}}));
      } else {
        if (!validQuestions(input.tool_response)) throw new Error('Unknown response shape');
        process.stdout.write(JSON.stringify({hookSpecificOutput:{hookEventName:'PostToolUse',
          updatedToolOutput:{...input.tool_response,answers:{[question]:'Blue'}}}}));
      }
    }
  });
} else { throw new Error('Expected init, observe, answer, rewrite or report'); }
