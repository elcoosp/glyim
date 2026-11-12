import { register } from 'node:module';
import { pathToFileURL } from 'node:url';

register(new URL('./svg-loader.mjs', import.meta.url));
