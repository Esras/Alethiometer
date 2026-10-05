// Settings (⌘,): today's behaviour, kept by the Modal component.
export const checks = {
  'Escape closes Settings': async (t) => {
    await t.steps('Control+,');
    await t.visible(t.page.getByPlaceholder('Find a setting'));
    await t.steps('Escape');
    await t.gone(t.page.getByPlaceholder('Find a setting'));
  },
};
