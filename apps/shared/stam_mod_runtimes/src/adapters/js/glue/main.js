// Staminal JavaScript Runtime Glue Code
// This file is loaded at runtime initialization and provides core APIs

// =============================================================================
// Console API - Format helpers
// =============================================================================

// Format argument for normal console output (no quotes on strings)
const __formatArg = (arg) =>{
    // 1. Special Case: Null and Undefined
    if (arg === null) return 'null';
    if (arg === undefined) return 'undefined';

    // 2. Special Case: Error Objects
    if (arg instanceof Error) {
        if (arg.stack) {
            return arg.name + ": " + arg.message + "\n" + arg.stack;
        }
        return arg.toString();
    }

    // 3. Special Case: Promises (like Node.js: Promise { <pending> })
    if (arg instanceof Promise) {
        return 'Promise { <pending> }';
    }

    // 4. Special Case: Functions
    if (typeof arg === 'function') return arg.toString();

    // 5. Objects - use JSON.stringify with type annotation for special types
    if (typeof arg === 'object') {
        // Check for typed arrays and other special objects
        const typeName = Object.prototype.toString.call(arg).slice(8, -1);

        // Handle ArrayBuffer and TypedArrays
        if (typeName === 'ArrayBuffer') {
            return `ArrayBuffer { byteLength: ${arg.byteLength} }`;
        }
        if (typeName.endsWith('Array') && typeName !== 'Array') {
            return `${typeName}(${arg.length}) [ ${Array.from(arg.slice(0, 10)).join(', ')}${arg.length > 10 ? ', ...' : ''} ]`;
        }

        // Handle Map
        if (arg instanceof Map) {
            const entries = Array.from(arg.entries()).slice(0, 10);
            const formatted = entries.map(([k, v]) => `${__formatArg(k)} => ${__formatArg(v)}`).join(', ');
            return `Map(${arg.size}) { ${formatted}${arg.size > 10 ? ', ...' : ''} }`;
        }

        // Handle Set
        if (arg instanceof Set) {
            const values = Array.from(arg.values()).slice(0, 10);
            const formatted = values.map(v => __formatArg(v)).join(', ');
            return `Set(${arg.size}) { ${formatted}${arg.size > 10 ? ', ...' : ''} }`;
        }

        // Handle Date
        if (arg instanceof Date) {
            return arg.toISOString();
        }

        // Handle RegExp
        if (arg instanceof RegExp) {
            return arg.toString();
        }

        // Regular objects - only own enumerable properties (like Node.js)
        try {
            return JSON.stringify(arg, null, 2);
        } catch (e) {
            return arg.toString();
        }
    }

    // 6. Primitives (Strings, Numbers, Booleans)
    return String(arg);
};

// Format argument for %o/%O - Node.js inspect style
// Strings are quoted, numbers/booleans are not, objects are formatted
const __inspectArg = (arg) => {
    // Null and undefined
    if (arg === null) return 'null';
    if (arg === undefined) return 'undefined';

    // Strings - quote with single quotes (like Node.js)
    if (typeof arg === 'string') return "'" + arg.replace(/'/g, "\\'") + "'";

    // Numbers, booleans - no quotes
    if (typeof arg === 'number' || typeof arg === 'boolean') return String(arg);

    // Functions
    if (typeof arg === 'function') return '[Function' + (arg.name ? ': ' + arg.name : '') + ']';

    // Error objects
    if (arg instanceof Error) {
        if (arg.stack) {
            return arg.name + ": " + arg.message + "\n" + arg.stack;
        }
        return arg.toString();
    }

    // Promises (like Node.js: Promise { <pending> })
    if (arg instanceof Promise) {
        return 'Promise { <pending> }';
    }

    // Arrays - format each element with inspect
    if (Array.isArray(arg)) {
        const items = arg.slice(0, 10).map(item => __inspectArg(item));
        return '[ ' + items.join(', ') + (arg.length > 10 ? ', ...' : '') + ' ]';
    }

    // Objects - format with keys and inspected values
    if (typeof arg === 'object') {
        // Check for typed arrays and other special objects
        const typeName = Object.prototype.toString.call(arg).slice(8, -1);

        // Handle ArrayBuffer and TypedArrays
        if (typeName === 'ArrayBuffer') {
            return `ArrayBuffer { byteLength: ${arg.byteLength} }`;
        }
        if (typeName.endsWith('Array') && typeName !== 'Array') {
            return `${typeName}(${arg.length}) [ ${Array.from(arg.slice(0, 10)).join(', ')}${arg.length > 10 ? ', ...' : ''} ]`;
        }

        // Handle Map
        if (arg instanceof Map) {
            const entries = Array.from(arg.entries()).slice(0, 10);
            const formatted = entries.map(([k, v]) => `${__inspectArg(k)} => ${__inspectArg(v)}`).join(', ');
            return `Map(${arg.size}) { ${formatted}${arg.size > 10 ? ', ...' : ''} }`;
        }

        // Handle Set
        if (arg instanceof Set) {
            const values = Array.from(arg.values()).slice(0, 10);
            const formatted = values.map(v => __inspectArg(v)).join(', ');
            return `Set(${arg.size}) { ${formatted}${arg.size > 10 ? ', ...' : ''} }`;
        }

        // Handle Date
        if (arg instanceof Date) {
            return arg.toISOString();
        }

        // Handle RegExp
        if (arg instanceof RegExp) {
            return arg.toString();
        }

        // Regular objects - only own enumerable properties (like Node.js)
        try {
            const keys = Object.keys(arg);
            if (keys.length === 0) return '{}';
            const displayKeys = keys.slice(0, 10);
            const pairs = displayKeys.map(k => {
                const val = arg[k];
                // Avoid infinite recursion
                if (val === arg) {
                    return k + ': [Circular]';
                }
                return k + ': ' + __inspectArg(val);
            });
            return '{ ' + pairs.join(', ') + (keys.length > 10 ? ', ...' : '') + ' }';
        } catch (e) {
            return arg.toString();
        }
    }

    return String(arg);
};

// Format arguments with printf-style placeholder support (%s, %d, %i, %f, %o, %O, %j, %%)
// This mimics Node.js console behavior
const __formatArgs = (...args) => {
    if (args.length === 0) return '';

    // If first arg is not a string, just format all args normally
    if (typeof args[0] !== 'string') {
        return args.map(__formatArg).join(' ');
    }

    const formatString = args[0];
    let argIndex = 1;
    let result = '';
    let i = 0;

    while (i < formatString.length) {
        if (formatString[i] === '%' && i + 1 < formatString.length) {
            const specifier = formatString[i + 1];

            // Check if we have an argument for this placeholder
            if (argIndex < args.length) {
                const arg = args[argIndex];

                switch (specifier) {
                    case 's': // String - no quotes
                        result += String(arg);
                        argIndex++;
                        i += 2;
                        continue;
                    case 'd': // Number (integer or float)
                    case 'i': // Integer
                        result += parseInt(arg, 10);
                        argIndex++;
                        i += 2;
                        continue;
                    case 'f': // Float
                        result += parseFloat(arg);
                        argIndex++;
                        i += 2;
                        continue;
                    case 'o': // Object (Node.js inspect style - strings quoted)
                    case 'O': // Object (same as %o)
                        result += __inspectArg(arg);
                        argIndex++;
                        i += 2;
                        continue;
                    case 'j': // JSON
                        try {
                            result += JSON.stringify(arg);
                        } catch (e) {
                            result += '[Circular]';
                        }
                        argIndex++;
                        i += 2;
                        continue;
                    case '%': // Literal %
                        result += '%';
                        i += 2;
                        continue;
                    default:
                        // Unknown specifier, output as-is
                        result += formatString[i];
                        i++;
                        continue;
                }
            } else if (specifier === '%') {
                // %% doesn't consume an argument
                result += '%';
                i += 2;
                continue;
            }
        }

        result += formatString[i];
        i++;
    }

    // Append any remaining arguments (like Node.js does)
    while (argIndex < args.length) {
        result += ' ' + __formatArg(args[argIndex]);
        argIndex++;
    }

    return result;
};

// =============================================================================
// Console API - Global object
// =============================================================================

globalThis.console = {
    log: (...args) => __console_native._log(__formatArgs(...args)),
    error: (...args) => __console_native._error(__formatArgs(...args)),
    warn: (...args) => __console_native._warn(__formatArgs(...args)),
    info: (...args) => __console_native._info(__formatArgs(...args)),
    debug: (...args) => __console_native._debug(__formatArgs(...args)),
    trace: (...args) => {
        // Generate stack trace
        const err = new Error();
        const stack = err.stack || '';
        // Remove the first line (Error) and the trace() call itself from the stack
        const stackLines = stack.split('\n').slice(2).join('\n');
        // Format message: "Trace: <message>\n<stack>"
        const message = args.length > 0 ? __formatArgs(...args) : '';
        const traceOutput = message ? `Trace: ${message}\n${stackLines}` : `Trace\n${stackLines}`;
        __console_native._log(traceOutput);
    },
};

// =============================================================================
// Error Handlers - Global error and unhandled promise rejection handlers
// =============================================================================

// Handler for uncaught errors
globalThis.onerror = (message, source, lineno, colno, error) => {
    const errorMsg = error ? (error.stack || error.message || String(error)) : message;
    __console_native._error(`Uncaught Error: ${errorMsg}`);
};

// Handler for unhandled promise rejections
globalThis.onunhandledrejection = (event) => {
    const reason = event && event.reason;
    let errorMsg;
    if (reason instanceof Error) {
        errorMsg = reason.stack || reason.message || String(reason);
    } else if (typeof reason === 'string') {
        errorMsg = reason;
    } else {
        try {
            errorMsg = JSON.stringify(reason);
        } catch {
            errorMsg = String(reason);
        }
    }
    __console_native._error(`Unhandled Promise Rejection: ${errorMsg}`);
};
