import { mount } from 'svelte';
import './theme/base.css';
import './theme/classic.css';
import './theme/modern.css';
import { look, setTheme } from './theme/theme.svelte';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('the page has no #app element');

setTheme(look.theme);
mount(App, { target });
