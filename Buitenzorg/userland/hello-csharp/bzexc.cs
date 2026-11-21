// Buitenzorg.Bcl -- the exception types of native (zerolib) C#.
//
// zerolib ships no System.Exception, so `throw` / `catch` did not compile.
// Every native program is built with this file (scripts/build-hello-csharp.*):
// the C# compiler finds System.Exception by name, the upstream ILC emits the
// unwind and clause tables, and the startup shim's dispatcher (bzeh.rs) runs
// them -- `try` / `catch` / `finally`, `catch (T e) when (...)`, `throw;`,
// exceptions thrown inside catch and finally blocks.
//
// System.IDisposable (for `using`) is added to zerolib itself by
// tools/ZerolibPatch: the compiler looks for it in the core library only.
//
// Keep `Message` the first field: an unhandled exception is reported by the
// shim as "Unhandled exception: <Message>", read from there.
//
// Only `throw` raises exceptions. The checks zerolib makes itself (array
// index, null, division by zero) still end the program with a message.
// (zerolib has no string concatenation, so messages are fixed strings.)

namespace System
{
    public class Exception
    {
        public string Message;
        public Exception InnerException;
        public Exception() { Message = "Exception of type System.Exception was thrown."; }
        public Exception(string message) { Message = message; }
        public Exception(string message, Exception inner) { Message = message; InnerException = inner; }
    }

    public class SystemException : Exception
    {
        public SystemException() : base("System error.") { }
        public SystemException(string message) : base(message) { }
        public SystemException(string message, Exception inner) : base(message, inner) { }
    }

    public class ApplicationException : Exception
    {
        public ApplicationException() : base("Error in the application.") { }
        public ApplicationException(string message) : base(message) { }
    }

    public class InvalidOperationException : SystemException
    {
        public InvalidOperationException() : base("Operation is not valid due to the current state of the object.") { }
        public InvalidOperationException(string message) : base(message) { }
        public InvalidOperationException(string message, Exception inner) : base(message, inner) { }
    }

    public class ArgumentException : SystemException
    {
        public string ParamName;
        public ArgumentException() : base("Value does not fall within the expected range.") { }
        public ArgumentException(string message) : base(message) { }
        public ArgumentException(string message, string paramName) : base(message) { ParamName = paramName; }
    }

    public class ArgumentNullException : ArgumentException
    {
        public ArgumentNullException() : base("Value cannot be null.") { }
        public ArgumentNullException(string paramName) : base("Value cannot be null.", paramName) { }
        public ArgumentNullException(string paramName, string message) : base(message, paramName) { }
    }

    public class ArgumentOutOfRangeException : ArgumentException
    {
        public ArgumentOutOfRangeException() : base("Specified argument was out of the range of valid values.") { }
        public ArgumentOutOfRangeException(string paramName) : base("Specified argument was out of the range of valid values.", paramName) { }
        public ArgumentOutOfRangeException(string paramName, string message) : base(message, paramName) { }
    }

    public class NotSupportedException : SystemException
    {
        public NotSupportedException() : base("Specified method is not supported.") { }
        public NotSupportedException(string message) : base(message) { }
    }

    public class NotImplementedException : SystemException
    {
        public NotImplementedException() : base("The method or operation is not implemented.") { }
        public NotImplementedException(string message) : base(message) { }
    }

    public class FormatException : SystemException
    {
        public FormatException() : base("One of the identified items was in an invalid format.") { }
        public FormatException(string message) : base(message) { }
    }

    public class IndexOutOfRangeException : SystemException
    {
        public IndexOutOfRangeException() : base("Index was outside the bounds of the array.") { }
        public IndexOutOfRangeException(string message) : base(message) { }
    }

    public class NullReferenceException : SystemException
    {
        public NullReferenceException() : base("Object reference not set to an instance of an object.") { }
        public NullReferenceException(string message) : base(message) { }
    }

    public class InvalidCastException : SystemException
    {
        public InvalidCastException() : base("Specified cast is not valid.") { }
        public InvalidCastException(string message) : base(message) { }
    }

    public class OverflowException : SystemException
    {
        public OverflowException() : base("Arithmetic operation resulted in an overflow.") { }
        public OverflowException(string message) : base(message) { }
    }

    public class DivideByZeroException : SystemException
    {
        public DivideByZeroException() : base("Attempted to divide by zero.") { }
        public DivideByZeroException(string message) : base(message) { }
    }

    public class TimeoutException : SystemException
    {
        public TimeoutException() : base("The operation has timed out.") { }
        public TimeoutException(string message) : base(message) { }
    }

    public class OperationCanceledException : SystemException
    {
        public OperationCanceledException() : base("The operation was canceled.") { }
        public OperationCanceledException(string message) : base(message) { }
    }

    public class ObjectDisposedException : InvalidOperationException
    {
        public string ObjectName;
        public ObjectDisposedException(string objectName) : base("Cannot access a disposed object.") { ObjectName = objectName; }
    }
}

namespace System.IO
{
    public class IOException : SystemException
    {
        public IOException() : base("I/O error occurred.") { }
        public IOException(string message) : base(message) { }
    }

    public class FileNotFoundException : IOException
    {
        public string FileName;
        public FileNotFoundException(string message) : base(message) { }
        public FileNotFoundException(string message, string fileName) : base(message) { FileName = fileName; }
    }

    public class EndOfStreamException : IOException
    {
        public EndOfStreamException() : base("Unable to read beyond the end of the stream.") { }
        public EndOfStreamException(string message) : base(message) { }
    }
}

namespace System.Collections.Generic
{
    public class KeyNotFoundException : SystemException
    {
        public KeyNotFoundException() : base("The given key was not present in the dictionary.") { }
        public KeyNotFoundException(string message) : base(message) { }
    }
}
