import { ComboBox, Description, FieldError, Input, Label, ListBox } from "@heroui/react";
import { type ReactNode, useMemo } from "react";
import { type Control, type FieldPath, type FieldValues, useController } from "react-hook-form";
import { timeZones } from "../lib/date";

interface TimeZoneInputProps<T extends FieldValues> {
  control: Control<T>;
  name: FieldPath<T>;
  label: ReactNode;
  description?: ReactNode;
}

interface Zone {
  id: string;
}

/** 时区（IANA 名字）选择，可以输入搜索。 */
export function TimeZoneInput<T extends FieldValues>(props: TimeZoneInputProps<T>) {
  const { field, fieldState } = useController({ control: props.control, name: props.name });
  const zones = useMemo<Zone[]>(() => timeZones().map((id) => ({ id })), []);
  const value = (field.value as string | undefined) ?? "";
  return (
    <ComboBox
      fullWidth
      name={field.name}
      defaultItems={zones}
      selectedKey={value || null}
      onSelectionChange={(key) => field.onChange(key === null ? "" : String(key))}
      isInvalid={fieldState.invalid}
      validationBehavior="aria"
    >
      <Label>{props.label}</Label>
      <ComboBox.InputGroup>
        <Input ref={field.ref} placeholder="输入搜索，例如 Shanghai" onBlur={field.onBlur} />
        <ComboBox.Trigger />
      </ComboBox.InputGroup>
      {props.description && !fieldState.error ? (
        <Description>{props.description}</Description>
      ) : null}
      <FieldError>{fieldState.error?.message}</FieldError>
      <ComboBox.Popover>
        <ListBox>
          {(item: object) => {
            const zone = item as Zone;
            return (
              <ListBox.Item id={zone.id} textValue={zone.id}>
                {zone.id}
                <ListBox.ItemIndicator />
              </ListBox.Item>
            );
          }}
        </ListBox>
      </ComboBox.Popover>
    </ComboBox>
  );
}
