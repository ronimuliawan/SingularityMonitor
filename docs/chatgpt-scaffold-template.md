# ChatGPT PRD Scaffold Template

---

**Created by SnapperAI**  
Visit **www.snapperai.io** for more AI tutorials | YouTube: **youtube.com/@snapperAI**

---

## Step 1: The PRD Prompt Builder

Copy and paste this exact prompt into ChatGPT to generate a complete Claude-ready PRD prompt:

```
I need you to create a PRD scaffold for my AI project, then format it as a complete Claude prompt.

Project: Singularity Monitor is a precision data-tracking utility for Windows 11 designed to deliver deep analytics with an industry-leading background footprint of under 5MB RAM. By utilizing a "differential tracking" approach, the app periodically polls native Windows connectivity APIs—acting like a digital odometer—to ensure 100% accuracy without the CPU-heavy burden of real-time packet sniffing. This design allows the app to instantly import up to two months of existing system history upon installation, so users never have to start their tracking from scratch.

The project employs a robust split-process architecture, where a tiny, headless daemon written in a systems-level language (like Rust or C++) handles data collection while a separate "Viewer" GUI provides on-demand insights. By archiving data in a local, optimized SQLite database, Singularity Monitor extends tracking far beyond the 60-day window provided by Windows, enabling advanced features like predictive cost-forecasting, usage heatmaps, and "AFK" audits to identify sneaky background updates. It is the definitive "anti-bloat" tool for power users who demand total transparency over their data without sacrificing system performance.

First, create a high-level scaffold with these sections:
- Executive Summary (key objectives, success metrics)
- Problem Statement (pain points, opportunity)
- Solution Overview (core concept, differentiators)
- User Personas (primary users, needs)
- Technical Architecture (components, stack, integrations)
- Functional Requirements (core features, user stories)
- Implementation Plan (MVP phases, timeline)
- Success Metrics (KPIs, targets)

Keep the scaffold structured but brief - bullet points and placeholders.

Then return your response formatted as a complete Claude prompt like this:

"You are an expert product manager and technical architect. Transform this PRD scaffold into a comprehensive, production-ready Product Requirements Document.

PROJECT SCAFFOLD:
[INCLUDE THE COMPLETE SCAFFOLD YOU CREATED HERE]

Expand this scaffold into a detailed PRD with these requirements:

1. EXECUTIVE SUMMARY
   - Vision and value proposition (2-3 compelling paragraphs)
   - Key objectives with specific metrics
   - Expected impact and success criteria

2. PROBLEM STATEMENT
   - Current market situation with data
   - User pain points with real scenarios
   - Opportunity size and cost of inaction

3. SOLUTION OVERVIEW
   - How the solution works (detailed explanation)
   - Technical approach and key decisions
   - Core differentiators

4. USER PERSONAS
   - 3 detailed personas with names, roles, workflows
   - Specific pain points and quotes
   - Technical proficiency levels

5. TECHNICAL ARCHITECTURE
   - Complete system components and technology stack
   - Data flow and integration specifications
   - Scalability and security considerations

6. FUNCTIONAL REQUIREMENTS
   - 10-15 detailed user stories with acceptance criteria
   - Feature priority levels (P0, P1, P2)
   - User flow descriptions

7. API SPECIFICATIONS
   - Key endpoints with methods and examples
   - Authentication and rate limiting
   - Error handling

8. DATA MODELS
   - Database schema with relationships
   - Data validation rules
   - Storage requirements

9. IMPLEMENTATION PLAN
   - Sprint breakdown with effort estimates
   - Development phases and dependencies
   - Team composition needs

10. SUCCESS METRICS
    - Specific KPIs with targets
    - Measurement methods and review intervals

Make it comprehensive enough for a development team to start building immediately. Use clear markdown formatting with tables where helpful."

Remember to include [INCLUDE MY PROJECT DESCRIPTION HERE] exactly where I described my project.
```

## How to Use This Template

1. **Replace the placeholder**: Change `[DESCRIBE YOUR AI PROJECT IN 1-2 SENTENCES]` with your actual project description
2. **Run it in ChatGPT**: Use GPT-4 or GPT-4 Turbo for best results
3. **Copy the entire response**: ChatGPT will give you a complete Claude prompt
4. **Paste directly into Claude**: No editing needed - just paste and run!

## Example Project Descriptions

- "An AI system that analyzes YouTube videos and automatically creates Twitter threads, LinkedIn posts, and newsletter content from them"
- "A customer service chatbot that integrates with Shopify to handle returns, track orders, and recommend products"  
- "An AI coding assistant that reviews pull requests and suggests improvements based on company coding standards"

## What You Get

ChatGPT will:
1. **Create a structured scaffold** for your specific project
2. **Wrap it in Claude instructions** with detailed expansion requirements
3. **Give you a complete prompt** ready to paste into Claude
4. **No additional editing needed** - just copy and paste

## Pro Tips

- Be specific about your target users and main goal
- Mention key integrations or platforms upfront
- Focus on the WHAT, not the HOW
- Use GPT-4 for best results

---

## Connect with SnapperAI

🌐 **Website:** [snapperai.io](https://www.snapperai.io)  
📺 **YouTube:** [youtube.com/@snapperAI](https://www.youtube.com/@snapperAI)  
🐦 **X/Twitter:** [x.com/SnapperSol](https://x.com/SnapperSol)  
📧 **Questions?** Reach out through any of our channels or email snapperdotsol@gmail.com

---

*© 2025 SnapperAI - PRD Generator System*