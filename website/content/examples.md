<p class="eyebrow">LEARN BY READING CODE</p>

# Examples

Small, complete programs for the current Skuld compiler. Copy a program to a `.skuld` file and run it locally.

<div class="filter-bar" role="group" aria-label="Filter examples"><button data-filter="all" aria-pressed="true">All examples</button><button data-filter="basics" aria-pressed="false">Basics</button><button data-filter="data" aria-pressed="false">Data structures</button><button data-filter="objects" aria-pressed="false">Objects</button><button data-filter="errors" aria-pressed="false">Error handling</button><button data-filter="files" aria-pressed="false">Files</button><button data-filter="http" aria-pressed="false">HTTP</button></div>

<section class="example-card" data-category="basics">

## Hello, Skuld

A typed binding, interpolation and a native entry point.

```skuld hello.skuld
func main() {
    let language = "Skuld"
    print("Hello, ${language}!")
}
```

[First program guide](/docs/getting-started/hello-world/) · <a href="/playground/" data-playground>Open in playground →</a>

</section>

<section class="example-card" data-category="data">

## Stable sorting

Sort a copy of a small array while leaving the original unchanged.

```skuld sorting.skuld
func main() {
    let numbers = [5, 3, 9, 1]
    let sorted = numbers.to_sorted((a, b) => a.compare(b))
    for number in sorted {
        print(number)
    }
}
```

`compare` returns an `Ordering`, so the comparator cannot overflow, unlike `a - b`.

[Arrays and sorting](/docs/language/arrays/) · <a href="/playground/" data-playground>Open in playground →</a>

</section>

<section class="example-card" data-category="objects">

## A shared object

Class methods use `this`; aliases share the same instance.

```skuld counter.skuld
class Counter {
    value: int = 0
    increment() {
        this.value += 1
    }
}

func main() {
    let counter = new Counter()
    let alias = counter
    alias.increment()
    print(counter.value)
}
```

[Class reference](/docs/language/classes/) · <a href="/playground/" data-playground>Open in playground →</a>

</section>

<section class="example-card" data-category="errors">

## Handle a result

Keep a recoverable failure visible in the function signature.

```skuld result.skuld
func divide(a: int, b: int) -> Result<int, string> {
    if b == 0 {
        return Err("divisor must be nonzero")
    }
    return Ok(a / b)
}

func main() {
    match divide(42, 2) {
        Ok(value): print(value)
        Err(error): print(error)
    }
}
```

This example handles zero; signed division overflow still traps.

[Error handling](/docs/language/error-handling/) · <a href="/playground/" data-playground>Open in playground →</a>

</section>

<section class="example-card" data-category="files">

## Read a text file

Read UTF-8 text or report the filesystem error. Create `notes.txt` beside the running program first.

```skuld read.skuld
import "std/fs"

func main() {
    match fs.read_text("notes.txt") {
        Ok(text): print(text)
        Err(error): print(fs.describe(error))
    }
}
```

[Filesystem APIs](/docs/standard-library/filesystem/) · <a href="/playground/" data-playground>Open in playground →</a>

</section>

<section class="example-card" data-category="http">

## Fetch a local response

This needs an HTTP server listening on `127.0.0.1:8080`. The call is blocking and handles connection failure as a Result.

```skuld request.skuld
import "std/http"

func main() {
    match http.get("http://127.0.0.1:8080/data.json") {
        Ok(response): print(response.body)
        Err(error): print(http.describe(error))
    }
}
```

[Networking APIs](/docs/standard-library/networking/) · <a href="/playground/" data-playground>Open in playground →</a>

</section>

## Looking for concurrency?

Threads and async/await are not implemented. Read the [concurrency status](/docs/advanced/concurrency/) rather than relying on proposed examples.
