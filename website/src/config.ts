export const site = {
  name: 'Skuld', version: '0.1.0',
  origin: process.env.SITE_URL || 'https://skuld-language-docs.yay9857.chatgpt.site',
  github: 'https://github.com/yay99857/skuld-lang',
};
export const groups = [
  { title: 'Getting started', path: 'getting-started', pages: ['introduction', 'installation', 'hello-world', 'project-structure'] },
  { title: 'The language', path: 'language', pages: ['syntax', 'variables', 'primitive-types', 'operators', 'control-flow', 'functions'] },
  { title: 'Types & data', path: 'language', pages: ['structs', 'classes', 'enums', 'interfaces', 'arrays', 'generics'] },
  { title: 'Organization', path: 'language', pages: ['modules', 'visibility'] },
  { title: 'Reliability', path: 'language', pages: ['error-handling', 'memory-management', 'defer'] },
  { title: 'Tooling', path: 'tooling', pages: ['compiler', 'cli', 'formatter', 'package-manager', 'editor-support'] },
  { title: 'Standard library', path: 'standard-library', pages: ['overview', 'strings', 'collections', 'io', 'filesystem', 'networking', 'json', 'math', 'time'] },
  { title: 'Advanced', path: 'advanced', pages: ['concurrency', 'interoperability', 'performance', 'compiler-internals', 'freestanding'] },
];
export const navigation = groups.flatMap(g => g.pages.map(slug => ({ group: g.title, slug, path: `/docs/${g.path}/${slug}/`, file: `${g.path}/${slug}.md` })));
