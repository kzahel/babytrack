import { mount } from 'svelte';
import App from './App.svelte';
import { setRelayOrigin } from '../../../core-wasm/web/relay-origin.js';
import './styles/index.css';

// A separately hosted build names its relay at build time; empty means same origin.
setRelayOrigin(import.meta.env.VITE_RELAY_ORIGIN || '');

mount(App, { target: document.getElementById('app') });
