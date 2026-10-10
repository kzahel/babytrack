<script>
  import { copy as c } from '../strings.js';
  import Icon from './Icon.svelte';

  // First run: brand, three promises, then the actions in priority order.
  let { addChild, join, restore, privacy } = $props();
</script>

<section class="welcome">
  <div class="welcome-hero">
    <span class="welcome-mark"><Icon name="mark" size={58} /></span>
    <h1>{c.app}</h1>
    <p class="tagline">{c.tagline}</p>
    <ul class="promises">
      <li><Icon name="no-account" size={22} /><div><b>{c.promiseAccount}</b><span>{c.promiseAccountDetail}</span></div></li>
      <li><Icon name="phone" size={22} /><div><b>{c.promiseLocal}</b><span>{c.promiseLocalDetail}</span></div></li>
      <li><Icon name="lock" size={22} /><div><b>{c.promiseShare}</b><span>{c.promiseShareDetail}</span></div></li>
    </ul>
  </div>
  <div class="welcome-actions">
    <button class="primary" onclick={addChild}>{c.addYourChild}</button>
    <button class="secondary" onclick={join}>{c.joinFamily}</button>
    <div class="welcome-links">
      <button class="text-action" onclick={restore}>{c.restoreLink}</button>
      <button class="text-action" onclick={privacy}>{c.privacy}</button>
    </div>
  </div>
</section>
