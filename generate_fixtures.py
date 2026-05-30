import os
from reportlab.lib.pagesizes import letter
from reportlab.pdfgen import canvas
from reportlab.lib.units import inch

def create_invoice(path):
    c = canvas.Canvas(path, pagesize=letter)
    c.setFont("Helvetica-Bold", 24)
    c.drawString(50, 750, "INVOICE")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 700, "Invoice Number:")
    c.setFont("Helvetica", 12)
    c.drawString(180, 700, "INV-2026-001")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 680, "Date:")
    c.setFont("Helvetica", 12)
    c.drawString(180, 680, "2026-05-23")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 630, "Bill To:")
    c.setFont("Helvetica", 12)
    c.drawString(50, 610, "Acme Corp")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 530, "Description")
    c.drawString(300, 530, "Quantity")
    c.drawString(400, 530, "Unit Price")
    c.drawString(500, 530, "Amount")
    
    c.setFont("Helvetica", 12)
    c.drawString(50, 500, "Cloud Database Hosting")
    c.drawString(300, 500, "1")
    c.drawString(400, 500, "$500.00")
    c.drawString(500, 500, "$500.00")
    
    c.drawString(50, 480, "Serverless Compute")
    c.drawString(300, 480, "10")
    c.drawString(400, 480, "$0.05")
    c.drawString(500, 480, "$0.50")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(400, 400, "Total:")
    c.drawString(500, 400, "$500.50")
    
    c.save()

def create_report(path):
    c = canvas.Canvas(path, pagesize=letter)
    c.setFont("Helvetica-Bold", 24)
    c.drawString(50, 750, "Quarterly Earnings Report")
    
    c.setFont("Helvetica-Bold", 14)
    c.drawString(50, 700, "Summary:")
    c.setFont("Helvetica", 12)
    c.drawString(50, 680, "This quarter we saw a 20% increase in recurring revenue.")
    
    c.setFont("Helvetica-Bold", 14)
    c.drawString(50, 630, "Metrics:")
    c.setFont("Helvetica", 12)
    c.drawString(50, 610, "Revenue: $1M")
    c.drawString(50, 590, "EBITDA: $200k")
    c.drawString(50, 570, "CAC: $150")
    c.save()

def create_form(path):
    c = canvas.Canvas(path, pagesize=letter)
    c.setFont("Helvetica-Bold", 24)
    c.drawString(50, 750, "Employee Intake Form")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 700, "First Name:")
    c.setFont("Helvetica", 12)
    c.drawString(150, 700, "John")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 670, "Last Name:")
    c.setFont("Helvetica", 12)
    c.drawString(150, 670, "Doe")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 640, "Department:")
    c.setFont("Helvetica", 12)
    c.drawString(150, 640, "Engineering")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 610, "Signature:")
    c.save()

os.makedirs("optimus-core/tests/fixtures", exist_ok=True)
create_invoice("optimus-core/tests/fixtures/invoice.pdf")
create_report("optimus-core/tests/fixtures/report.pdf")
create_form("optimus-core/tests/fixtures/form.pdf")

print("Created 3 PDF fixtures.")
