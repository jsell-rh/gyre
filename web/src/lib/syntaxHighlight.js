/**
 * Lightweight syntax highlighter — language detection + regex tokenizer.
 * Zero runtime dependencies. Produces HTML strings safe for {@html}.
 */

const EXT_LANG = {
  rs:'rust',js:'javascript',jsx:'javascript',ts:'typescript',tsx:'typescript',
  mjs:'javascript',cjs:'javascript',py:'python',go:'go',toml:'toml',json:'json',
  yaml:'yaml',yml:'yaml',html:'html',svelte:'html',css:'css',scss:'css',
  sh:'shell',bash:'shell',zsh:'shell',fish:'shell',nix:'nix',lua:'lua',
  rb:'ruby',java:'java',kt:'kotlin',c:'c',h:'c',cpp:'cpp',hpp:'cpp',
  cs:'csharp',sql:'sql',
};

const KEYWORDS = {
  rust: new Set(['as','async','await','break','const','continue','crate','dyn','else','enum','extern','false','fn','for','if','impl','in','let','loop','match','mod','move','mut','pub','ref','return','self','Self','static','struct','super','trait','true','type','unsafe','use','where','while','Option','Result','None','Some','Ok','Err','Box','Vec','String','str','bool','i8','i16','i32','i64','i128','u8','u16','u32','u64','u128','f32','f64','usize','isize']),
  javascript: new Set(['async','await','break','case','catch','class','const','continue','debugger','default','delete','do','else','export','extends','false','finally','for','function','if','import','in','instanceof','let','new','null','of','return','static','super','switch','this','throw','true','try','typeof','undefined','var','void','while','with','yield','from']),
  typescript: new Set(['abstract','as','async','await','break','case','catch','class','const','continue','debugger','declare','default','delete','do','else','enum','export','extends','false','finally','for','function','if','implements','import','in','instanceof','interface','is','keyof','let','module','namespace','new','null','of','override','private','protected','public','readonly','return','satisfies','static','super','switch','this','throw','true','try','type','typeof','undefined','var','void','while','yield','from','infer','never','unknown','any']),
  python: new Set(['and','as','assert','async','await','break','class','continue','def','del','elif','else','except','False','finally','for','from','global','if','import','in','is','lambda','None','not','or','pass','raise','return','True','try','while','with','yield']),
  go: new Set(['break','case','chan','const','continue','default','defer','else','fallthrough','for','func','go','goto','if','import','interface','map','nil','package','range','return','select','struct','switch','true','false','type','var','any','error','string','bool','int','int8','int16','int32','int64','uint','uint8','uint16','uint32','uint64','uintptr','float32','float64','complex64','complex128','byte','rune']),
  nix: new Set(['let','in','rec','with','inherit','if','then','else','assert','import','null','true','false','or','and']),
  ruby: new Set(['begin','break','case','class','def','do','else','elsif','end','ensure','false','for','if','in','module','next','nil','not','or','redo','rescue','retry','return','self','super','then','true','undef','unless','until','when','while','yield']),
  java: new Set(['abstract','assert','boolean','break','byte','case','catch','char','class','const','continue','default','do','double','else','enum','extends','false','final','finally','float','for','goto','if','implements','import','instanceof','int','interface','long','native','new','null','package','private','protected','public','return','short','static','strictfp','super','switch','synchronized','this','throw','throws','transient','true','try','void','volatile','while']),
  kotlin: new Set(['abstract','actual','as','break','by','catch','class','companion','const','constructor','continue','data','do','else','enum','expect','external','false','final','finally','for','fun','if','import','in','infix','init','inline','inner','interface','internal','is','lateinit','null','object','open','operator','out','override','package','private','protected','public','return','sealed','super','suspend','this','throw','true','try','typealias','val','value','var','vararg','when','where','while']),
  c: new Set(['auto','break','case','char','const','continue','default','do','double','else','enum','extern','float','for','goto','if','inline','int','long','register','restrict','return','short','signed','sizeof','static','struct','switch','typedef','union','unsigned','void','volatile','while','NULL','true','false']),
  cpp: new Set(['alignas','alignof','and','auto','bitand','bitor','bool','break','case','catch','char','class','compl','concept','const','consteval','constexpr','constinit','const_cast','continue','co_await','co_return','co_yield','decltype','default','delete','do','double','dynamic_cast','else','enum','explicit','export','extern','false','float','for','friend','goto','if','inline','int','long','mutable','namespace','new','noexcept','not','nullptr','operator','or','private','protected','public','register','reinterpret_cast','requires','return','short','signed','sizeof','static','static_assert','static_cast','struct','switch','template','this','thread_local','throw','true','try','typedef','typeid','typename','union','unsigned','using','virtual','void','volatile','wchar_t','while','xor']),
  csharp: new Set(['abstract','as','async','await','base','bool','break','byte','case','catch','char','checked','class','const','continue','decimal','default','delegate','do','double','else','enum','event','explicit','extern','false','finally','fixed','float','for','foreach','goto','if','implicit','in','int','interface','internal','is','lock','long','namespace','new','null','object','operator','out','override','params','private','protected','public','readonly','ref','return','sbyte','sealed','short','sizeof','stackalloc','static','string','struct','switch','this','throw','true','try','typeof','uint','ulong','unchecked','unsafe','ushort','using','virtual','void','volatile','while','var','dynamic','record']),
  sql: new Set(['ADD','ALL','ALTER','AND','AS','ASC','BETWEEN','BY','CASE','COLUMN','CONSTRAINT','CREATE','CROSS','DATABASE','DEFAULT','DELETE','DESC','DISTINCT','DROP','ELSE','END','EXISTS','FOREIGN','FROM','FULL','GROUP','HAVING','INNER','INSERT','INTO','IS','JOIN','KEY','LEFT','LIKE','LIMIT','NOT','NULL','OFFSET','ON','OR','ORDER','OUTER','PRIMARY','REFERENCES','RIGHT','SELECT','SET','TABLE','THEN','TRUNCATE','UNION','UNIQUE','UPDATE','VALUES','VIEW','WHEN','WHERE','WITH','add','all','alter','and','as','asc','between','by','case','column','constraint','create','cross','database','default','delete','desc','distinct','drop','else','end','exists','foreign','from','full','group','having','inner','insert','into','is','join','key','left','like','limit','not','null','offset','on','or','order','outer','primary','references','right','select','set','table','then','truncate','union','unique','update','values','view','when','where','with']),
};

const HASH_COMMENT_LANGS = new Set(['python','ruby','shell','toml','yaml','nix']);
const SLASH_COMMENT_LANGS = new Set(['rust','javascript','typescript','go','c','cpp','csharp','java','kotlin']);

function escapeHtml(s) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

export function detectLang(filePath) {
  const name = filePath.split('/').pop() ?? '';
  if (name==='Makefile'||name==='makefile'||name==='GNUmakefile') return 'shell';
  if (name==='Dockerfile') return 'shell';
  const ext = name.includes('.') ? name.split('.').pop().toLowerCase() : '';
  return EXT_LANG[ext] ?? 'text';
}

function tokenizeLine(line, lang) {
  const tokens = [];
  const kws = KEYWORDS[lang];
  let i = 0;
  while (i < line.length) {
    const ch = line[i];
    if (HASH_COMMENT_LANGS.has(lang) && ch === '#') {
      tokens.push({ type:'comment', content:line.slice(i) });
      return tokens;
    }
    if (SLASH_COMMENT_LANGS.has(lang) && ch==='/' && line[i+1]==='/') {
      tokens.push({ type:'comment', content:line.slice(i) });
      return tokens;
    }
    if (ch==='/' && line[i+1]==='*') {
      const end = line.indexOf('*/', i+2);
      if (end !== -1) { tokens.push({ type:'comment', content:line.slice(i,end+2) }); i=end+2; }
      else { tokens.push({ type:'comment', content:line.slice(i) }); return tokens; }
      continue;
    }
    if (ch==='"' || ch==="'" || ch==='`') {
      let j=i+1;
      while (j<line.length) { if(line[j]==='\\'){j+=2;continue;} if(line[j]===ch){j++;break;} j++; }
      tokens.push({ type:'string', content:line.slice(i,j) }); i=j; continue;
    }
    if (/\d/.test(ch) && (i===0 || /\W/.test(line[i-1]))) {
      let j=i;
      if (ch==='0' && (line[i+1]==='x'||line[i+1]==='X')) { j+=2; while(j<line.length&&/[0-9a-fA-F_]/.test(line[j]))j++; }
      else { while(j<line.length&&/[\d._eE]/.test(line[j]))j++; const m=line.slice(j).match(/^(u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize|f32|f64)/); if(m)j+=m[0].length; }
      tokens.push({ type:'number', content:line.slice(i,j) }); i=j; continue;
    }
    if (/[a-zA-Z_$]/.test(ch)) {
      let j=i; while(j<line.length&&/\w/.test(line[j]))j++;
      const word=line.slice(i,j);
      tokens.push({ type:kws?.has(word)?'keyword':'ident', content:word }); i=j; continue;
    }
    if (/\s/.test(ch)) {
      let j=i+1; while(j<line.length&&/\s/.test(line[j]))j++;
      tokens.push({ type:'plain', content:line.slice(i,j) }); i=j; continue;
    }
    tokens.push({ type:'plain', content:ch }); i++;
  }
  return tokens;
}

function highlightJson(content) {
  const tokens = [];
  let i=0;
  while (i<content.length) {
    if (content[i]==='"') {
      let j=i+1;
      while(j<content.length){if(content[j]==='\\'){j+=2;continue;}if(content[j]==='"'){j++;break;}j++;}
      tokens.push({type:'string',content:content.slice(i,j)}); i=j; continue;
    }
    if (/[-\d]/.test(content[i])&&(i===0||/[^"\w]/.test(content[i-1]))) {
      const m=content.slice(i).match(/^-?\d+(\.\d+)?([eE][+-]?\d+)?/);
      if(m){tokens.push({type:'number',content:m[0]});i+=m[0].length;continue;}
    }
    if (/[tfn]/.test(content[i])) {
      const lit=['true','false','null'].find(k=>content.startsWith(k,i));
      if(lit){tokens.push({type:'keyword',content:lit});i+=lit.length;continue;}
    }
    tokens.push({type:'plain',content:content[i]}); i++;
  }
  return tokens.map(({type,content:c})=>{
    const esc=escapeHtml(c);
    if(type==='keyword') return `<span class="hl-kw">${esc}</span>`;
    if(type==='string') return `<span class="hl-str">${esc}</span>`;
    if(type==='number') return `<span class="hl-num">${esc}</span>`;
    return esc;
  }).join('');
}

export function highlightLine(content, lang) {
  if (!lang || lang==='text') return escapeHtml(content);
  if (lang==='json') return highlightJson(content);
  return tokenizeLine(content,lang).map(({type,content:c})=>{
    const esc=escapeHtml(c);
    if(type==='keyword') return `<span class="hl-kw">${esc}</span>`;
    if(type==='string') return `<span class="hl-str">${esc}</span>`;
    if(type==='comment') return `<span class="hl-cmt">${esc}</span>`;
    if(type==='number') return `<span class="hl-num">${esc}</span>`;
    return esc;
  }).join('');
}
