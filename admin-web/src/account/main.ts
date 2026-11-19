import '@fontsource-variable/inter/opsz.css';
import '../styles/tokens.css';
import '../styles/admin.css';
import { mount } from 'svelte';
import Account from './Account.svelte';

mount(Account, { target: document.getElementById('app')! });
