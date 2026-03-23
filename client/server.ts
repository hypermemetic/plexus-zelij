import indexHtml from './index.html';

console.log('🌐 Locus Web Viewer starting on http://localhost:3000');
console.log('🔌 Connecting to RPC server at ws://127.0.0.1:44480');

Bun.serve({
  port: 3000,
  routes: {
    '/': indexHtml,
  },
  development: {
    hmr: true,
    console: true,
  },
});
