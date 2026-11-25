/*
 * MppToMspdi.java: the whole MPXJ "sidecar" program.
 *
 * WHAT THIS FILE IS FOR
 * Microsoft Project saves its plans in ".mpp" files. The .mpp format is
 * secret (Microsoft has never published it), so we cannot easily read it
 * from Rust. The open-source Java library MPXJ (https://www.mpxj.org) has
 * spent 20+ years learning to read it. This tiny program uses MPXJ to turn
 * an .mpp file into "MSPDI": Microsoft Project's own, publicly documented
 * XML format. Ganttora's Rust code then reads the XML.
 *
 * WHERE IT FITS IN THE PROJECT
 * This program is a "sidecar": a separate helper program that ships next
 * to the main app and is started by it, like a sidecar attached to a
 * motorcycle. The Rust code in backend/crates/interop/src/mpp/sidecar.rs
 * (milestone M3) will start it, give it a file path, and read what it prints.
 *
 *     Ganttora (Rust)                       mpxj-sidecar (this program)
 *     ----------------                      ---------------------------
 *     start "mpxj-sidecar shed.mpp"  ---->  read shed.mpp with MPXJ
 *                                           convert to MSPDI XML
 *     read the XML from stdout       <----  print the XML to stdout
 *     read error text from stderr    <----  print problems to stderr
 *     check the exit code            <----  exit with 0 (ok) or 1..3 (error)
 *
 * MAIN IDEAS TO KNOW FIRST
 * - "stdout" (standard output) and "stderr" (standard error) are the two
 *   text channels every program has. We keep stdout clean: ONLY the XML is
 *   written there, so Rust can read it without filtering. All messages for
 *   humans go to stderr.
 * - The "exit code" is a small number a program returns when it ends.
 *   0 means success; anything else means an error. The codes used here are
 *   listed in the constants below.
 * - In the shipped app this Java code is compiled by GraalVM "native-image"
 *   into a normal program (.exe on Windows), so users do not need Java.
 */

// A "package" is a folder-like name that groups Java classes and avoids name
// clashes. By convention it is the project's web domain written backwards.
package com.ganttora.mpxj;

// "import" lines make classes from other packages available by short name.
import java.io.BufferedOutputStream;
import java.io.File;
import java.io.IOException;
import java.io.OutputStream;

import org.mpxj.MPXJException;
import org.mpxj.ProjectFile;
import org.mpxj.mspdi.MSPDIWriter;
import org.mpxj.reader.UniversalProjectReader;

/**
 * Command-line program that converts one project file (usually .mpp) into
 * MSPDI XML printed on standard output.
 *
 * <p><b>Usage:</b> {@code mpxj-sidecar <path-to-file.mpp>}
 *
 * <p><b>Beginner note:</b> in Java, all code lives inside a "class". A class
 * marked {@code final} cannot be extended by other classes; we use it because
 * this class is a complete program, not a building block.
 */
public final class MppToMspdi {

    /** Exit code: the conversion worked and the XML is on stdout. */
    static final int EXIT_OK = 0;

    /** Exit code: the program was started with the wrong arguments. */
    static final int EXIT_USAGE = 1;

    /**
     * Exit code: the input file could not be read (missing, unreadable,
     * damaged, password-protected, or not a project file MPXJ understands).
     */
    static final int EXIT_READ_FAILED = 2;

    /** Exit code: the file was read, but writing the XML failed. */
    static final int EXIT_WRITE_FAILED = 3;

    /**
     * Private constructor: nobody needs to create an object of this class,
     * because everything happens in the static {@link #main} method.
     */
    private MppToMspdi() {
    }

    /**
     * Program entry point: Java starts every program by calling {@code main}.
     *
     * <p><b>What it does:</b> checks the arguments, runs {@link #convert}, and
     * ends the program with the exit code that {@code convert} returned.
     *
     * <p><b>Parameters:</b> {@code args} are the words typed after the program
     * name on the command line. We expect exactly one: the input file path.
     *
     * <p><b>Returns:</b> nothing; the result is the exit code and the output.
     *
     * <p><b>Example:</b> {@code mpxj-sidecar shed.mpp > shed.xml}
     * (the {@code >} sends stdout into the file shed.xml).
     *
     * @param args command-line arguments: one path to the input file
     */
    public static void main(String[] args) {
        // System.exit ends the program and hands the exit code to whoever
        // started it (the Rust code, or the terminal).
        System.exit(convert(args, System.out));
    }

    /**
     * Converts the project file named in {@code args} to MSPDI XML.
     *
     * <p><b>What it does:</b>
     * <ol>
     *   <li>Checks there is exactly one argument and that the file exists.</li>
     *   <li>Reads it with MPXJ's {@code UniversalProjectReader}, which looks
     *       at the file's first bytes and picks the right reader (.mpp, .mpx,
     *       MSPDI, Primavera …) by itself.</li>
     *   <li>Writes the project as MSPDI XML to {@code out}.</li>
     * </ol>
     *
     * <p><b>Parameters:</b>
     * <ul>
     *   <li>{@code args}: the command-line arguments (see {@link #main}).</li>
     *   <li>{@code out}: where the XML goes; {@code System.out} in real use.
     *       Passing it in (instead of using {@code System.out} directly) lets
     *       tests capture the output.</li>
     * </ul>
     *
     * <p><b>Returns:</b> one of the {@code EXIT_*} codes.
     *
     * <p><b>Errors:</b> never throws. Every problem is reported as one line on
     * stderr starting with {@code "mpxj-sidecar: "}, plus a non-zero code.
     *
     * <p><b>Beginner note:</b> Java reports many errors by "throwing an
     * exception", which jumps out of the normal flow until a {@code catch}
     * block handles it. Here we catch exceptions and turn them into exit
     * codes, because a separate program cannot throw an exception to Rust.
     *
     * @param args command-line arguments
     * @param out  stream that receives the XML
     * @return an exit code (0 = success)
     */
    static int convert(String[] args, OutputStream out) {
        if (args.length != 1) {
            System.err.println("mpxj-sidecar: usage: mpxj-sidecar <path-to-project-file>");
            return EXIT_USAGE;
        }

        File input = new File(args[0]);
        // Check early so the user gets a clear message instead of a long
        // technical error from deep inside MPXJ.
        if (!input.isFile()) {
            System.err.println("mpxj-sidecar: file not found: " + input.getPath());
            return EXIT_USAGE;
        }

        // Step 1: read the file into MPXJ's in-memory model (ProjectFile).
        ProjectFile project;
        try {
            project = new UniversalProjectReader().read(input);
        } catch (MPXJException e) {
            System.err.println("mpxj-sidecar: could not read " + input.getPath() + ": " + e.getMessage());
            return EXIT_READ_FAILED;
        } catch (RuntimeException e) {
            // A damaged file can make a reader fail in unexpected ways.
            // We still want a clean error message, not a crash.
            System.err.println("mpxj-sidecar: unexpected error while reading " + input.getPath() + ": " + e);
            return EXIT_READ_FAILED;
        }
        // UniversalProjectReader returns null (= "no value") when it does not
        // recognise the file type at all.
        if (project == null) {
            System.err.println("mpxj-sidecar: not a recognised project file: " + input.getPath());
            return EXIT_READ_FAILED;
        }

        // Step 2: write MSPDI XML. A BufferedOutputStream collects bytes in
        // memory and writes them in large blocks, which is much faster than
        // writing many tiny pieces. "try (...)" closes it automatically and
        // flushes (sends) the last buffered bytes.
        try (OutputStream buffered = new BufferedOutputStream(new NonClosingStream(out))) {
            new MSPDIWriter().write(project, buffered);
        } catch (IOException | RuntimeException e) {
            System.err.println("mpxj-sidecar: could not write MSPDI XML: " + e);
            return EXIT_WRITE_FAILED;
        }
        return EXIT_OK;
    }

    /**
     * Wraps a stream so that closing the wrapper only flushes it.
     *
     * <p><b>Why:</b> closing {@code System.out} would make later messages
     * vanish and is a common source of confusing bugs. The buffered writer
     * above closes whatever it wraps, so we give it this wrapper instead.
     */
    private static final class NonClosingStream extends java.io.FilterOutputStream {

        /**
         * Creates the wrapper.
         *
         * @param target the real stream (stays open)
         */
        NonClosingStream(OutputStream target) {
            super(target);
        }

        /**
         * Writes a block of bytes straight to the target. (The parent class
         * would write them one byte at a time, which is slow.)
         */
        @Override
        public void write(byte[] b, int off, int len) throws IOException {
            out.write(b, off, len);
        }

        /** Flushes the target but leaves it open. */
        @Override
        public void close() throws IOException {
            out.flush();
        }
    }
}
