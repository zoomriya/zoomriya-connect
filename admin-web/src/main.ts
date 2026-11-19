import '@fontsource-variable/inter/opsz.css';
import './styles/tokens.css';
import './styles/admin.css';
import { mount } from 'svelte';
import App from './App.svelte';

mount(App, { target: document.getElementById('app')! });
