<script lang="ts">
  import { enhance } from '$app/forms';

  let { form } = $props();
</script>

<div class="login-container">
  <div class="login-card">
    <h1>postgraph</h1>
    <form method="POST" use:enhance>
      {#if form?.error}
        <p class="error">{form.error}</p>
      {/if}
      <input
        type="password"
        name="password"
        placeholder="Password"
        autocomplete="current-password"
        required
      />
      <button type="submit">Sign in</button>
    </form>
  </div>
</div>

<style>
  .login-container {
    display: flex;
    justify-content: center;
    align-items: center;
    height: 100vh;
    background: #0a0a0a;
  }
  .login-card {
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 2rem;
    width: 320px;
    text-align: center;
  }
  h1 {
    margin: 0 0 1.5rem;
    color: #eee;
    font-size: 1.5rem;
  }
  input {
    width: 100%;
    padding: 0.6rem;
    background: #1a1a1a;
    border: 1px solid #444;
    color: #eee;
    border-radius: 4px;
    font-size: 1rem;
    margin-bottom: 1rem;
    box-sizing: border-box;
  }
  button {
    width: 100%;
    padding: 0.6rem;
    background: #4363d8;
    border: none;
    color: white;
    border-radius: 4px;
    font-size: 1rem;
    cursor: pointer;
  }
  button:hover { background: #3751b5; }
  .error {
    color: #e6194b;
    font-size: 0.85rem;
    margin: 0 0 1rem;
  }
</style>
