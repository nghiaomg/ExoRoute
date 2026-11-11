import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import Input from '../../src/components/Input.svelte';
import ArkField from '../../src/components/ArkField.svelte';

describe('shared Input', () => {
  test('preserves the standard aria-label attribute', () => {
    render(Input, { props: { 'aria-label': 'Gateway name', required: true } });
    expect(screen.getByRole('textbox', { name: 'Gateway name' }).hasAttribute('required')).toBe(true);
  });
  test('binds text and number values using native input semantics', async () => {
    const textChange = vi.fn();
    const numberChange = vi.fn();
    const text = render(Input, { props: { type: 'text', ariaLabel: 'Name', value: 'initial', oninput: textChange } });
    const textInput = screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement;
    await fireEvent.input(textInput, { target: { value: 'updated' } });
    expect(textInput.value).toBe('updated');
    expect(textChange).toHaveBeenCalledTimes(1);
    text.unmount();

    render(Input, { props: { type: 'number', ariaLabel: 'Limit', value: 3, oninput: numberChange } });
    const numberInput = screen.getByRole('spinbutton', { name: 'Limit' }) as HTMLInputElement;
    await fireEvent.input(numberInput, { target: { value: '4' } });
    expect(numberInput.valueAsNumber).toBe(4);
    expect(numberChange).toHaveBeenCalledTimes(1);
  });

  test('updates checked state and forwards native checkbox changes', async () => {
    const onChange = vi.fn();
    render(Input, { props: { type: 'checkbox', ariaLabel: 'Enabled', checked: false, onchange: onChange } });
    const checkbox = screen.getByRole('checkbox', { name: 'Enabled' }) as HTMLInputElement;
    await fireEvent.click(checkbox);
    expect(checkbox.checked).toBe(true);
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  test('keeps the native file element reference and forwards file changes', async () => {
    const onChange = vi.fn();
    render(Input, { props: { type: 'file', ariaLabel: 'Upload', onchange: onChange } });
    const fileInput = screen.getByLabelText('Upload') as HTMLInputElement;
    expect(fileInput.type).toBe('file');
    await fireEvent.change(fileInput);
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  test('preserves field context, label and input callback', async () => {
    const onValueChange = vi.fn();
    render(ArkField, { props: { label: 'Model ID', value: '', onValueChange } });
    const field = screen.getByRole('textbox', { name: 'Model ID' });
    await fireEvent.input(field, { target: { value: 'provider/model' } });
    expect(onValueChange).toHaveBeenCalledWith('provider/model');
  });
});
