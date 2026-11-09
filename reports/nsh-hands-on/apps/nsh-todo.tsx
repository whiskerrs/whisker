import { computed, mount, Scroll, signal, Text, View } from "nsh";
import { Button, Input } from "nsh/base";

type Todo = { id: number; title: string; done: boolean };
type Filter = "all" | "active" | "done";

const todos = signal<Todo[]>([
  { id: 1, title: "Read the NSH docs", done: true },
  { id: 2, title: "Build a todo app", done: false },
  { id: 3, title: "Compare with Whisker", done: false },
]);

function toggle(id: number) {
  todos.update((list) => list.map((t) => (t.id === id ? { ...t, done: !t.done } : t)));
}

function remove(id: number) {
  todos.update((list) => list.filter((t) => t.id !== id));
}

function TodoRow(props: { item: () => Todo }) {
  return (
    <View class="flex-row items-center gap-3 rounded-[10px] bg-[#1A1A22] px-4 py-3">
      <Button onPress={() => toggle(props.item().id)} class="flex-1 flex-row items-center gap-3">
        <View
          class={`size-[22px] rounded-full border-2 border-[#7C5CFF] ${props.item().done ? "bg-[#7C5CFF]" : "bg-[#1A1A22]"}`}
        />
        <Text class={`flex-1 text-base ${props.item().done ? "text-[#8A8A99]" : "text-white"}`}>
          {props.item().title}
        </Text>
      </Button>
      <Button onPress={() => remove(props.item().id)} class="px-2">
        <Text class="text-sm text-[#8A8A99]">Delete</Text>
      </Button>
    </View>
  );
}

function FilterTab(props: { label: string; value: Filter; filter: () => Filter; set: (f: Filter) => void }) {
  return (
    <Button
      onPress={() => props.set(props.value)}
      class={`flex-1 items-center rounded-lg py-2 ${props.filter() === props.value ? "bg-[#7C5CFF]" : "bg-[#1A1A22]"}`}
    >
      <Text class="text-base font-bold text-white">{props.label}</Text>
    </Button>
  );
}

function App() {
  const draft = signal("");
  const filter = signal<Filter>("all");
  let nextId = 4;
  const submit = () => {
    const title = draft().trim();
    if (!title) return;
    todos.update((list) => [...list, { id: nextId++, title, done: false }]);
    draft.set("");
  };
  const visible = computed(() =>
    todos().filter((t) => (filter() === "all" ? true : filter() === "active" ? !t.done : t.done)),
  );
  const left = computed(() => todos().filter((t) => !t.done).length);

  return (
    <View class="flex-1 gap-3 bg-[#0B0B0F] px-4 pt-[calc(env(safe-area-inset-top)+16px)] pb-[env(safe-area-inset-bottom)]">
      <View class="flex-row items-center justify-between">
        <Text class="text-[28px] text-white">Todos</Text>
        <Text class="text-base text-[#8A8A99]">{left()} left</Text>
      </View>
      <View class="flex-row items-center gap-2">
        <Input
          value={draft()}
          onChange={({ value }) => draft.set(value)}
          onSubmit={submit}
          placeholder="What needs doing?"
          class="flex-1 rounded-lg bg-[#1A1A22] px-3 py-2 text-base text-white"
        />
        <Button onPress={submit} class="rounded-lg bg-[#7C5CFF] px-4 py-2 active:opacity-60">
          <Text class="text-base text-white">Add</Text>
        </Button>
      </View>
      <View class="flex-row gap-2">
        <FilterTab label="All" value="all" filter={filter} set={(f) => filter.set(f)} />
        <FilterTab label="Active" value="active" filter={filter} set={(f) => filter.set(f)} />
        <FilterTab label="Done" value="done" filter={filter} set={(f) => filter.set(f)} />
      </View>
      <Scroll class="flex-1">
        <View class="gap-2">
          <Each of={visible()} key={(t) => t.id}>
            {(item) => <TodoRow item={item} />}
          </Each>
        </View>
      </Scroll>
    </View>
  );
}

mount(App);
