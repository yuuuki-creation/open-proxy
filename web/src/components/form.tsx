// react-hook-form 和 HeroUI 表单组件的桥接：值和校验交给 react-hook-form（zod 规则），
// HeroUI 只负责显示；字段一律用 aria 校验方式，不触发浏览器自带的提示框。

import {
  Checkbox,
  CheckboxGroup,
  Description,
  FieldError,
  Input,
  Label,
  ListBox,
  Radio,
  RadioGroup,
  Select,
  Switch,
  TextArea,
  TextField,
} from "@heroui/react";
import type { HTMLAttributes, ReactNode } from "react";
import { type Control, type FieldPath, type FieldValues, useController } from "react-hook-form";

interface FieldProps<T extends FieldValues> {
  control: Control<T>;
  name: FieldPath<T>;
  label: ReactNode;
  description?: ReactNode;
  isRequired?: boolean;
  isDisabled?: boolean;
  className?: string;
}

export interface Option {
  id: string;
  label: string;
  description?: ReactNode;
  isDisabled?: boolean;
}

interface TextInputProps<T extends FieldValues> extends FieldProps<T> {
  type?: "text" | "password" | "url" | "email" | "date" | "search";
  placeholder?: string;
  autoComplete?: string;
  inputMode?: HTMLAttributes<HTMLInputElement>["inputMode"];
  autoFocus?: boolean;
  /** 等宽字体（Token、地址等） */
  mono?: boolean;
}

export function TextInput<T extends FieldValues>(props: TextInputProps<T>) {
  const { control, name, label, description, isRequired, isDisabled, className } = props;
  const { field, fieldState } = useController({ control, name });
  return (
    <TextField
      className={className}
      fullWidth
      name={field.name}
      type={props.type ?? "text"}
      value={(field.value as string | undefined) ?? ""}
      onChange={field.onChange}
      onBlur={field.onBlur}
      isInvalid={fieldState.invalid}
      isRequired={isRequired}
      isDisabled={isDisabled}
      validationBehavior="aria"
      autoFocus={props.autoFocus}
    >
      <Label>{label}</Label>
      <Input
        ref={field.ref}
        className={props.mono ? "font-mono" : undefined}
        placeholder={props.placeholder}
        autoComplete={props.autoComplete ?? "off"}
        inputMode={props.inputMode}
      />
      {description && !fieldState.error ? <Description>{description}</Description> : null}
      <FieldError>{fieldState.error?.message}</FieldError>
    </TextField>
  );
}

interface TextAreaInputProps<T extends FieldValues> extends FieldProps<T> {
  placeholder?: string;
  rows?: number;
  mono?: boolean;
}

export function TextAreaInput<T extends FieldValues>(props: TextAreaInputProps<T>) {
  const { control, name, label, description, isRequired, isDisabled, className } = props;
  const { field, fieldState } = useController({ control, name });
  return (
    <TextField
      className={className}
      fullWidth
      name={field.name}
      value={(field.value as string | undefined) ?? ""}
      onChange={field.onChange}
      onBlur={field.onBlur}
      isInvalid={fieldState.invalid}
      isRequired={isRequired}
      isDisabled={isDisabled}
      validationBehavior="aria"
    >
      <Label>{label}</Label>
      <TextArea
        ref={field.ref}
        className={props.mono ? "font-mono text-xs" : undefined}
        placeholder={props.placeholder}
        rows={props.rows ?? 4}
        spellCheck={false}
      />
      {description && !fieldState.error ? <Description>{description}</Description> : null}
      <FieldError>{fieldState.error?.message}</FieldError>
    </TextField>
  );
}

interface SelectInputProps<T extends FieldValues> extends FieldProps<T> {
  options: Option[];
  placeholder?: string;
}

/** 单选下拉框；表单里存选项 id（字符串），空字符串表示没选。 */
export function SelectInput<T extends FieldValues>(props: SelectInputProps<T>) {
  const { control, name, label, description, isRequired, isDisabled, className, options } = props;
  const { field, fieldState } = useController({ control, name });
  const value = field.value as string | undefined;
  return (
    <Select
      className={className}
      fullWidth
      name={field.name}
      placeholder={props.placeholder ?? "请选择"}
      value={value ? value : null}
      onChange={(key) => field.onChange(key === null ? "" : String(key))}
      onBlur={field.onBlur}
      isInvalid={fieldState.invalid}
      isRequired={isRequired}
      isDisabled={isDisabled}
      validationBehavior="aria"
      disabledKeys={options.filter((o) => o.isDisabled).map((o) => o.id)}
    >
      <Label>{label}</Label>
      <Select.Trigger ref={field.ref}>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      {description && !fieldState.error ? <Description>{description}</Description> : null}
      <FieldError>{fieldState.error?.message}</FieldError>
      <Select.Popover>
        <ListBox>
          {options.map((option) => (
            <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
              {option.label}
              <ListBox.ItemIndicator />
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  );
}

interface RadioInputProps<T extends FieldValues> extends FieldProps<T> {
  options: Option[];
  orientation?: "horizontal" | "vertical";
}

export function RadioInput<T extends FieldValues>(props: RadioInputProps<T>) {
  const { control, name, label, description, isRequired, isDisabled, className, options } = props;
  const { field, fieldState } = useController({ control, name });
  return (
    <RadioGroup
      className={className}
      name={field.name}
      value={(field.value as string | undefined) ?? ""}
      onChange={field.onChange}
      isInvalid={fieldState.invalid}
      isRequired={isRequired}
      isDisabled={isDisabled}
      validationBehavior="aria"
      orientation={props.orientation ?? "vertical"}
    >
      <Label>{label}</Label>
      {description ? <Description>{description}</Description> : null}
      {options.map((option) => (
        <Radio key={option.id} value={option.id} isDisabled={option.isDisabled}>
          <Radio.Content>
            <Radio.Control>
              <Radio.Indicator />
            </Radio.Control>
            {option.label}
          </Radio.Content>
          {option.description ? <Description>{option.description}</Description> : null}
        </Radio>
      ))}
      <FieldError>{fieldState.error?.message}</FieldError>
    </RadioGroup>
  );
}

interface SwitchInputProps<T extends FieldValues> {
  control: Control<T>;
  name: FieldPath<T>;
  label: ReactNode;
  description?: ReactNode;
  isDisabled?: boolean;
}

export function SwitchInput<T extends FieldValues>(props: SwitchInputProps<T>) {
  const { field } = useController({ control: props.control, name: props.name });
  return (
    <Switch
      name={field.name}
      isSelected={Boolean(field.value)}
      onChange={field.onChange}
      isDisabled={props.isDisabled}
    >
      <Switch.Content>
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        {props.label}
      </Switch.Content>
      {props.description ? <Description>{props.description}</Description> : null}
    </Switch>
  );
}

interface CheckboxListInputProps<T extends FieldValues> extends FieldProps<T> {
  options: Option[];
}

/** 多选（例如套餐的可用节点）；表单里存选中的 id 数组。 */
export function CheckboxListInput<T extends FieldValues>(props: CheckboxListInputProps<T>) {
  const { control, name, label, description, isDisabled, className, options } = props;
  const { field, fieldState } = useController({ control, name });
  return (
    <CheckboxGroup
      className={className}
      name={field.name}
      value={(field.value as string[] | undefined) ?? []}
      onChange={field.onChange}
      isInvalid={fieldState.invalid}
      isDisabled={isDisabled}
      validationBehavior="aria"
    >
      <Label>{label}</Label>
      {description ? <Description>{description}</Description> : null}
      {options.map((option) => (
        <Checkbox key={option.id} value={option.id} isDisabled={option.isDisabled}>
          <Checkbox.Content>
            <Checkbox.Control>
              <Checkbox.Indicator />
            </Checkbox.Control>
            {option.label}
          </Checkbox.Content>
          {option.description ? <Description>{option.description}</Description> : null}
        </Checkbox>
      ))}
      <FieldError>{fieldState.error?.message}</FieldError>
    </CheckboxGroup>
  );
}
